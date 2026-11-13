    #[test]
    fn pruning_preserves_predecessor_successor_dependencies() {
        let (state,root)=state();
        let path="src/engine/memory.rs";
        let method=json!({"id":"program","revision":"p1","name":"three steps",
            "domain":"memory","assistanceMode":"harness_assisted",
            "verification":{"type":"procedure",
                "appliesTo":{"requestContains":["improve"]},
                "nodes":[
                    {"id":"read","tool":"workspace_read_range",
                        "arguments":{"path":path,"startLine":1,"endLine":1}},
                    {"id":"search","tool":"code_search",
                        "arguments":{"query":"index"}},
                    {"id":"edit","tool":"workspace_replace",
                        "arguments":{"path":path,"startLine":1,"endLine":1,
                            "expectedSha256":"$read.sourceSha256",
                            "newText":"fn optimized() {}\n"}}],
                "edges":[{"from":"read","to":"search"},{"from":"search","to":"edit"}],
                "after":[{"kind":"file_contains","path":path,
                    "text":"fn optimized()"}]}});
        state.store.put("rsi_candidate",&json!({"id":"accept",
            "status":"verified","paths":[path],
            "evaluation":{"metrics":{"eligible":true}}})).unwrap();
        let op=|id:&str,node:&str,tool:&str,args:Value,result:Value|json!({
            "id":id,"status":"completed","origin":"procedure",
            "procedureStrategyId":"program","procedureRevision":"p1",
            "procedureNodeId":node,"tool":tool,"arguments":args,"result":result});
        let ops=vec![
            op("op1","read","workspace_read_range",
                json!({}),json!({"sourceSha256":"a".repeat(64)})),
            op("op2","search","code_search",
                json!({}),json!({"matches":[{"path":path}]})),
            op("op3","edit","workspace_replace",
                json!({"path":path}),json!({"written":true,
                    "postSha256":"b".repeat(64)}))
        ];
        let receipt=json!({"methodId":"program","revision":"p1",
            "executionId":"run","operationIds":["op1","op2","op3"],
            "result":{"verified":true,"followed":true}});
        let generation=json!({"jobId":"generation","accepted":true,
            "candidateId":"accept",
            "acceptedSourceHashes":{"src/engine/memory.rs":"b".repeat(64)}});
        let proposed=synthesize_observed_dag_pruning(
            &state,&method,&receipt,&ops,&generation).unwrap();
        assert_eq!(proposed["verification"]["edges"],
            json!([{"from":"read","to":"edit"}]),
            "the edit still waits for the source read after pruning search");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn accepted_parent_tries_compacted_child_once_then_uses_observed_outcome() {
        let (state,root)=state();
        let input=json!({"rsiSourceDir":root.join("source").to_string_lossy(),
            "rsiObjective":"memory improve"});
        let scope=research_scope(&input).partition_key;
        let parent=json!({"id":"parent","revision":"p1","status":"executable",
            "assistanceMode":"harness_assisted","name":"parent",
            "domain":"memory","successfulApplications":2,
            "applications":[{"accepted":true},{"accepted":true}],
            "verification":{"type":"procedure",
                "appliesTo":{"requestContains":["memory"]},
                "nodes":[{"id":"read","tool":"workspace_read_range",
                    "arguments":{"path":"src/engine/memory.rs","startLine":1}},
                    {"id":"edit","tool":"workspace_replace","arguments":{
                    "path":"src/engine/memory.rs","startLine":1,"endLine":1,
                    "expectedSha256":"$read.sourceSha256",
                    "newText":"fn original() {}"}}],
                "edges":[{"from":"read","to":"edit"}],
                "after":[{"kind":"file_exists","path":"src/engine/memory.rs"}]}});
        let child=json!({"id":"child","revision":"c1","status":"candidate",
            "assistanceMode":"harness_assisted","name":"compact child",
            "domain":"memory","successfulApplications":0,
            "verification":{"type":"procedure",
                "appliesTo":{"requestContains":["memory"]},
                "nodes":[{"id":"edit","tool":"workspace_replace","arguments":{
                    "path":"src/engine/memory.rs","startLine":1,"endLine":1,
                    "expectedSha256":null,"newText":"fn better() {}"}}],
                "after":[{"kind":"file_contains","path":"src/engine/memory.rs",
                    "text":"fn better()"}]},
            "synthesis":{"kind":"successful_read_ablation",
                "parentMethodId":"parent","parentRevision":"p1"}});
        state.store.put("experience",&json!({"id":"success-lineage",
            "partitionKey":scope,"status":"active",
            "executableMethods":[parent,child]})).unwrap();
        let first=method_reuse_decision(&state,&input,"memory");
        assert_eq!(first["repairMethodId"],"child",
            "a previously successful parent must actually test its untried child");
        assert_eq!(first["repairSelectionEvidence"]["selection"],"success_ablation_trial");
        let mut history=state.store.get("experience","success-lineage").unwrap();
        history["executableMethods"][1]["applications"]=json!([{
            "accepted":false,"verificationVerdict":"passed"}]);
        history["executableMethods"][1]["status"]=json!("executable");
        state.store.put("experience",&history).unwrap();
        let second=method_reuse_decision(&state,&input,"memory");
        assert!(second["methodIds"].as_array().unwrap().iter().any(|id|id=="parent"));
        assert_eq!(second["repairMethodId"],Value::Null,
            "unadopted child no longer has priority over proven parent");
        // An adoption must have an actual accepted, postcondition-passing
        // application receipt. A counter alone is not proof of a child gain.
        history["executableMethods"][1]["applications"]=json!([{
            "accepted":true,"postconditionsPassed":true,"verificationVerdict":"passed"
        }]);
        history["executableMethods"][1]["successfulApplications"]=json!(1);
        state.store.put("experience",&history).unwrap();
        let third=method_reuse_decision(&state,&input,"memory");
        assert_eq!(third["repairMethodId"],"child",
            "once the improved descendant is itself adopted, prefer it");
        history["executableMethods"][0]["successfulApplications"]=json!(100);
        state.store.put("experience",&history).unwrap();
        let promoted=method_reuse_decision(&state,&input,"memory");
        assert_eq!(promoted["repairMethodId"],"child",
            "cumulative historical uses must not permanently block adopted descendants");
        assert_eq!(promoted["repairSelectionEvidence"]["selection"],
            "adopted_structural_descendant");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn zero_patch_model_failure_learns_a_new_descendant_tool_program() {
        let (state,root)=state();
        let path="src/engine/memory.rs";
        let objective="fix memory";
        let session=json!({"id":"failed-model-session","rsiRun":true,
            "rsiGenerationJobId":"failed-generation",
            "rsiObjective":objective,"rsiFocusFiles":[path]});
        let gen=json!({"jobId":"failed-generation","domain":"memory",
            "accepted":false,"newFindings":0,"tokens":250,"elapsedMs":50});
        let op=json!({"id":"failed-model-edit","origin":"model",
            "sessionId":"failed-model-session","status":"completed",
            "tool":"workspace_replace","arguments":{"path":path,
                "startLine":3,"endLine":3,
                "expectedSha256":"stale","newText":"fn improved() {}"},
            "result":{"error":"읽은 뒤 소스가 변경되었습니다",
                "staleVersion":true,"written":false}});
        let repair=synthesize_failed_model_edit(&session,&op,&gen).unwrap();
        assert_eq!(repair["synthesis"]["kind"],"failed_model_tool_rebase");
        assert_eq!(repair["status"],"candidate");
        assert_eq!(repair["successfulApplications"],0);
        assert_ne!(repair["id"],Value::Null);
        let mut results=std::collections::HashMap::new();
        let (read_id,tool,args)=crate::engine::procedure::ready_intent(
            &repair,&[],objective,&results).unwrap();
        assert_eq!(tool,"workspace_read_range");
        assert_eq!(args["path"],path);
        assert_eq!(args["startLine"],3);
        results.insert(read_id.clone(),json!({"sourceSha256":"fresh-current-sha"}));
        let witnessed=json!({"origin":"procedure","status":"completed",
            "procedureStrategyId":repair["id"],"procedureRevision":repair["revision"],
            "procedureNodeId":read_id,"result":{"sourceSha256":"fresh-current-sha"}});
        let (_,tool,args)=crate::engine::procedure::ready_intent(
            &repair,&[witnessed],objective,&results).unwrap();
        assert_eq!(tool,"workspace_replace");
        assert_eq!(args["expectedSha256"],"fresh-current-sha");
        assert_eq!(args["newText"],"fn improved() {}");
        // No original method was ever proposed by the model. This cold-start
        // repair is created from the raw failed tool receipt alone.
        state.store.put("session",&session).unwrap();
        state.store.put("execution_operation",&op).unwrap();
        let feedback=record_method_feedback(&state,&gen,"failed-model-session");
        assert!(feedback.is_empty(),
            "there was no method execution, only an observed model failure");
        let saved=state.store.get("session","failed-model-session").unwrap();
        assert_eq!(saved["rsiMethodProposals"][0]["id"],repair["id"]);
        let input=json!({"rsiSourceDir":root.join("source").to_string_lossy(),
            "rsiObjective":objective});
        let experience=remember_research(
            &state,&input,&gen,"failed-model-session").unwrap();
        assert_eq!(state.store.get("experience",&experience).unwrap()
            ["executableMethods"][0]["status"],"candidate");
        let selected=method_reuse_decision(&state,&input,"memory");
        assert_eq!(selected["repairMethodId"],repair["id"]);
        assert_eq!(selected["repairTarget"],path);
        assert_eq!(selected["repairSelectionEvidence"]["verifiedAdoptions"],0);
        // A different session, missing stale evidence, or unbound path
        // cannot manufacture a reusable procedure from model prose.
        let mut bad=op.clone();
        bad["result"]["staleVersion"]=json!(false);
        assert!(synthesize_failed_model_edit(&session,&bad,&gen).is_none());
        bad=op.clone();
        bad["arguments"]["path"]=json!("src/engine/unobserved.rs");
        assert!(synthesize_failed_model_edit(&session,&bad,&gen).is_none());
        let mut blocked=op.clone();
        blocked["result"]["staleVersion"]=json!(false);
        blocked["result"]["preflightBlocked"]=json!(true);
        assert!(synthesize_failed_model_edit(&session,&blocked,&gen).is_some(),
            "observed preflight blocks should produce a re-read-before-edit method");
        bad=op.clone();
        bad["origin"]=json!("procedure");
        assert!(synthesize_failed_model_edit(&session,&bad,&gen).is_none());
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn genuine_reversed_model_span_evolves_into_versioned_descendant_program() {
        let (state,root)=state();
        let path="src/engine/memory.rs";
        let session=json!({"id":"reversed-s","rsiRun":true,
            "rsiGenerationJobId":"g1","rsiObjective":"fix memory",
            "rsiFocusFiles":[path]});
        let generation=json!({"jobId":"g1","accepted":false,
            "newFindings":0,"tokens":2400});
        let failure=json!({"id":"edit-reversed","origin":"model",
            "sessionId":"reversed-s","status":"failed",
            "tool":"workspace_replace","arguments":{"path":path,
                "startLine":"14","endLine":"12",
                "newText":"let improved = true;"},
            "result":{"error":"endLine은 startLine 이상이어야 합니다.",
                "written":false}});
        let learned=synthesize_failed_reversed_span(
            &session,&failure,&generation).unwrap();
        assert_eq!(learned["synthesis"]["kind"],"failed_model_reversed_span");
        assert_eq!(learned["status"],"candidate");
        assert_eq!(learned["successfulApplications"],0);
        assert_eq!(learned["synthesis"]["qualityGain"],Value::Null);
        let mut results=std::collections::HashMap::new();
        let (read_id,read_tool,read_args)=crate::engine::procedure::ready_intent(
            &learned,&[],"fix memory",&results).unwrap();
        assert_eq!(read_tool,"workspace_read_range");
        assert_eq!(read_args["startLine"],12);
        assert_eq!(read_args["endLine"],14);
        results.insert(read_id.clone(),json!({"sourceSha256":"witnessed-version"}));
        let receipt=json!({"origin":"procedure","status":"completed",
            "procedureStrategyId":learned["id"],
            "procedureRevision":learned["revision"],"procedureNodeId":read_id,
            "result":{"sourceSha256":"witnessed-version"}});
        let (_,edit_tool,edit_args)=crate::engine::procedure::ready_intent(
            &learned,&[receipt],"fix memory",&results).unwrap();
        assert_eq!(edit_tool,"workspace_replace");
        assert_eq!(edit_args["expectedSha256"],"witnessed-version");
        assert_eq!(edit_args["startLine"],12);
        assert_eq!(edit_args["endLine"],14);
        assert!(is_synthesized_source_repair(&learned));
        state.store.put("session",&session).unwrap();
        state.store.put("execution_operation",&failure).unwrap();
        record_method_feedback(&state,&generation,"reversed-s");
        let saved=state.store.get("session","reversed-s").unwrap();
        assert_eq!(saved["rsiMethodProposals"][0]["id"],learned["id"]);
        let input=json!({"rsiSourceDir":root.join("source").to_string_lossy(),
            "rsiObjective":"fix memory"});
        let experience=remember_research(&state,&input,&generation,"reversed-s").unwrap();
        let selected=method_reuse_decision(&state,&input,"memory");
        assert_eq!(selected["repairMethodId"],learned["id"]);
        assert_eq!(state.store.get("experience",&experience).unwrap()
            ["executableMethods"][0]["status"],"candidate");
        let mut unrelated=failure.clone();
        unrelated["sessionId"]=json!("other");
        assert!(synthesize_failed_reversed_span(&session,&unrelated,&generation).is_none());
        unrelated=failure.clone();
        unrelated["arguments"]["endLine"]=json!(0);
        assert!(synthesize_failed_reversed_span(&session,&unrelated,&generation).is_none());
        unrelated=failure.clone();
        unrelated["result"]["error"]=json!("no change");
        assert!(synthesize_failed_reversed_span(&session,&unrelated,&generation).is_none());
        unrelated=failure.clone();
        unrelated["arguments"]["startLine"]=json!(140);
        assert!(synthesize_failed_reversed_span(&session,&unrelated,&generation).is_none());
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn witnessed_model_timeout_can_run_one_inherited_fix_not_repeat_full_research() {
        let failed=json!({"actionObservation":{"failedEditCalls":2},
            "editActions":[{"status":"failed","written":false}],
            "candidateId":null,"accepted":false});
        assert!(retry_learned_model_failure(
            "실행 시간 제한(10분)을 초과했습니다.",&failed,true,0,300,1,4));
        assert!(!retry_learned_model_failure(
            "실행 시간 제한(10분)을 초과했습니다.",&failed,false,0,300,1,4));
        assert!(!retry_learned_model_failure(
            "실행 시간 제한(10분)을 초과했습니다.",&failed,true,1,300,1,4));
        assert!(!retry_learned_model_failure(
            "실행 시간 제한(10분)을 초과했습니다.",&failed,true,0,119,1,4));
        assert!(!retry_learned_model_failure(
            "실행 시간 제한(10분)을 초과했습니다.",&failed,true,0,300,4,4));
        assert!(!retry_learned_model_failure(
            "provider authentication failed",&failed,true,0,300,1,4));
        let never_tried=json!({"actionObservation":{"failedEditCalls":0},"editActions":[]});
        assert!(!retry_learned_model_failure(
            "model timeout",&never_tried,true,0,300,1,4));
    }

    #[test]
    fn exhausted_model_recovery_changes_the_next_generation_strategy() {
        let episode=json!({"id":"generation-one","attempt":1,
            "accepted":false,"candidateId":null,"newFindings":4,
            "domain":"memory","actionDirective":{"mode":"discover"},
            "actionObservation":{"status":"research_only","sourceDiffBytes":0,
                "editToolCalls":0,"appliedEditCalls":0},
            "selfRepair":{"attempts":1,
                "modes":["source_edit_bootstrap"],
                "outcome":"no_patch_after_recovery",
                "qualityGain":null}});
        assert_eq!(action_learning_context(&episode),"exhausted_recovery");
        let adapted=next_execution_action(&[episode.clone()]);
        assert_eq!(adapted["mode"],"change_approach",
            "repeated search after a failed source-edit retry must not be the default");
        assert!(action_choices("exhausted_recovery").contains(&"change_approach"));
        let mut untried=episode.clone();
        untried["selfRepair"]["outcome"]=json!("not_attempted");
        assert_eq!(action_learning_context(&untried),"research_only");
        assert_eq!(next_execution_action(&[untried])["mode"],"implement_research");
        let mut successful=episode;
        successful["selfRepair"]["outcome"]=json!("candidate_after_recovery");
        successful["candidateId"]=json!("candidate-one");
        assert_eq!(action_learning_context(&successful),"unadopted_candidate");
        assert_eq!(next_execution_action(&[successful])["mode"],"revise_candidate");
    }

    #[test]
    fn failed_method_evolves_into_a_separate_inherited_candidate() {
        let (state,root)=state();
        let input=json!({"rsiSourceDir":root.join("source").to_string_lossy(),
            "rsiObjective":"memory improvement"});
        let partition=research_scope(&input).partition_key;
        let original=json!({"id":"old-method","revision":"old-revision",
            "name":"old source edit","domain":"memory","status":"executable",
            "assistanceMode":"harness_assisted","verification":{
                "type":"procedure","appliesTo":{"requestContains":["memory"]},
                "nodes":[{"id":"edit","tool":"workspace_replace","arguments":{
                    "path":"src/engine/memory.rs","startLine":1,"endLine":1,
                    "newText":"fn fixed() {}","expectedSha256":"outdated"}}],
                "after":[{"kind":"tool_called","tool":"workspace_replace",
                    "path":"src/engine/memory.rs"}]}});
        state.store.put("experience",&json!({"id":"prior-method",
            "status":"active","partitionKey":partition,
            "executableMethods":[original.clone()]})).unwrap();
        state.store.put("execution_operation",&json!({"id":"failed-edit",
            "sessionId":"s","origin":"procedure","status":"failed",
            "procedureStrategyId":"old-method","procedureRevision":"old-revision",
            "procedureNodeId":"edit","tool":"workspace_replace",
            "arguments":{"path":"src/engine/memory.rs","startLine":1,"endLine":1,
                "expectedSha256":"outdated","newText":"fn fixed() {}"},
            "result":{"error":"stale SHA","staleVersion":true}})).unwrap();
        state.store.put("session",&json!({"id":"s","rsiMethodExecutions":[{
            "experienceId":"prior-method","methodId":"old-method",
            "revision":"old-revision","executionId":"e","operationIds":["failed-edit"],
            "result":{"verified":true,"status":"violated","violations":["failed"]}
        }]})).unwrap();
        let generation=json!({"jobId":"generation-1","domain":"memory",
            "accepted":false,"newFindings":0,"tokens":100,"elapsedMs":50});
        record_method_feedback(&state,&generation,"s");
        assert_eq!(state.store.get("experience","prior-method").unwrap()
            ["executableMethods"][0]["status"],"suspended");
        let proposed=state.store.get("session","s").unwrap()
            ["rsiMethodProposals"][0].clone();
        assert_eq!(proposed["status"],"candidate");
        assert_eq!(proposed["sourceGenerationJobId"],"generation-1");
        assert_eq!(proposed["successfulApplications"],0);
        let saved=remember_research(&state,&input,&generation,"s").unwrap();
        assert_eq!(state.store.get("experience",&saved).unwrap()
            ["executableMethods"][0]["id"],proposed["id"]);
        let decision=method_reuse_decision(&state,&input,"memory");
        assert_eq!(decision["action"],"execute_methods");
        assert_eq!(decision["methodIds"][0],proposed["id"]);
        assert_ne!(decision["methodIds"][0],original["id"]);
        let _=std::fs::remove_dir_all(root);
    }

