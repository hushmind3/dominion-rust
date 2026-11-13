    #[test]
    fn proven_ablation_operator_transfers_to_new_capability_without_gain_credit() {
        let (state,root)=state();
        let source=root.join("source");
        std::fs::create_dir_all(source.join("src/engine")).unwrap();
        std::fs::write(source.join("src/engine/tools.rs"),"fn tool() {}\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&source).unwrap();
        let input=json!({"rsiSourceDir":source,"rsiObjective":"tools improve"});
        state.store.put("job",&json!({"id":"tools-generation",
            "kind":"rsi_generation","status":"running","input":{
                "rsiSourceDir":source,"rsiIndexId":index["id"]}})).unwrap();
        let donor=json!({"id":"adopted-memory-ablation","revision":"m1",
            "status":"executable","domain":"memory","successfulApplications":1,
            "applications":[{"accepted":true,"postconditionsPassed":true}],
            "synthesis":{"kind":"successful_read_ablation"}});
        state.store.put("experience",&json!({"id":"memory-donor",
            "status":"active","partitionKey":research_scope(&input).partition_key,
            "executableMethods":[donor]})).unwrap();
        let target=json!({"id":"target-tools","revision":"t1","name":"improve tools",
            "status":"executable","domain":"tools",
            "assistanceMode":"harness_assisted",
            "verification":{"type":"procedure",
                "appliesTo":{"requestContains":["tools"]},
                "nodes":[{"id":"read","tool":"workspace_read_range",
                    "arguments":{"path":"src/engine/tools.rs","startLine":1,"endLine":1}},
                    {"id":"edit","tool":"workspace_replace",
                    "arguments":{"path":"src/engine/tools.rs","startLine":1,
                        "endLine":1,"newText":"fn improved() {}",
                        "expectedSha256":"$read.sourceSha256"}}],
                "edges":[{"from":"read","to":"edit"}],
                "after":[{"kind":"file_contains","path":"src/engine/tools.rs",
                    "text":"fn improved()"}]}});
        let receipt=json!({"executionId":"tools-execution",
            "operationIds":["tools-read","tools-edit"],
            "result":{"verified":true,"status":"followed","followed":true}});
        let ops=vec![
            json!({"id":"tools-read","status":"completed","origin":"procedure",
                "tool":"workspace_read_range","procedureNodeId":"read",
                "procedureStrategyId":"target-tools","procedureRevision":"t1",
                "result":{"sourceSha256":"a".repeat(64)}}),
            json!({"id":"tools-edit","status":"completed","origin":"procedure",
                "tool":"workspace_replace","procedureNodeId":"edit",
                "procedureStrategyId":"target-tools","procedureRevision":"t1",
                "arguments":{"path":"src/engine/tools.rs",
                    "expectedSha256":"a".repeat(64)},
                "result":{"written":true}})];
        let generation=json!({"jobId":"tools-generation","accepted":false});
        let transferred=synthesize_successful_dag_ablation(
            &state,&target,&receipt,&ops,&generation).unwrap();
        assert_eq!(transferred["synthesis"]["kind"],"transferred_read_ablation");
        assert_eq!(transferred["synthesis"]["transferDonor"]["domain"],"memory");
        assert_eq!(transferred["synthesis"]["transferDonor"]["methodId"],
            "adopted-memory-ablation");
        assert_eq!(transferred["successfulApplications"],0);
        assert_eq!(transferred["synthesis"]["qualityGain"],Value::Null);
        let (_,tool,args)=crate::engine::procedure::ready_intent(
            &transferred,&[],"tools improve",&std::collections::HashMap::new()).unwrap();
        assert_eq!(tool,"workspace_replace");
        assert!(args["expectedSha256"].is_null());
        // A previously accepted method in another domain is an operator
        // hypothesis, not proof the target method's quality has improved.
        state.store.put("experience",&json!({"id":"tools-methods",
            "status":"active","partitionKey":research_scope(&input).partition_key,
            "executableMethods":[target.clone(),transferred]})).unwrap();
        let decision=method_reuse_decision(&state,&input,"tools");
        assert_eq!(decision["repairSelectionEvidence"]["selection"],
            "success_ablation_trial");
        assert_eq!(decision["repairTarget"],"src/engine/tools.rs");
        // A descendant that later gets adopted can teach the same operator
        // to a third domain. Every transfer is a fresh, uncredited candidate.
        let mut unrelated=state.store.get("experience","memory-donor").unwrap();
        unrelated["executableMethods"][0]["synthesis"]["kind"]=
            json!("transferred_read_ablation");
        unrelated["executableMethods"][0]["synthesis"]["transferDepth"]=json!(1);
        state.store.put("experience",&unrelated).unwrap();
        let second_hop=synthesize_successful_dag_ablation(
            &state,&target,&receipt,&ops,&generation).unwrap();
        assert_eq!(second_hop["synthesis"]["transferDepth"],2);
        assert_eq!(second_hop["synthesis"]["qualityGain"],Value::Null);
        unrelated["executableMethods"][0]["synthesis"]["transferDepth"]=json!(3);
        state.store.put("experience",&unrelated).unwrap();
        assert!(synthesize_successful_dag_ablation(
            &state,&target,&receipt,&ops,&generation).is_none(),
            "unbounded reuse of the same donor is not recursive proof");
        unrelated["executableMethods"][0]["synthesis"]["transferDepth"]=json!(1);
        unrelated["partitionKey"]=json!("rsi:foreign-repository");
        state.store.put("experience",&unrelated).unwrap();
        assert!(synthesize_successful_dag_ablation(
            &state,&target,&receipt,&ops,&generation).is_none(),
            "cross-repository donor inheritance is forbidden");
        unrelated["partitionKey"]=research_scope(&input).partition_key.into();
        unrelated["executableMethods"][0]["successfulApplications"]=json!(0);
        state.store.put("experience",&unrelated).unwrap();
        assert!(synthesize_successful_dag_ablation(
            &state,&target,&receipt,&ops,&generation).is_none(),
            "a donor with no accepted descendant cannot train the meta operator");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn accepted_program_rewrites_its_own_research_dag_and_preserves_live_dataflow() {
        for (domain,investigation,path) in [
            ("memory","code_search","src/engine/memory.rs"),
            ("tools","http_fetch","src/engine/tools.rs"),
        ] {
            let (state,root)=state();
            let method=json!({"id":"accepted-method","revision":"parent-v1",
                "domain":domain,"name":"research then edit",
                "assistanceMode":"harness_assisted",
                "verification":{"type":"procedure",
                    "appliesTo":{"requestContains":["improve"]},
                    "nodes":[
                        {"id":"research","tool":investigation,"arguments":
                            if investigation=="code_search" {
                                json!({"query":"memory"})
                            }else{json!({"url":"https://example.org"})}},
                        {"id":"read","tool":"workspace_read_range","arguments":{
                            "path":path,"startLine":1,"endLine":1}},
                        {"id":"edit","tool":"workspace_replace","arguments":{
                            "path":path,"startLine":1,"endLine":1,
                            "expectedSha256":"$read.sourceSha256",
                            "newText":"fn improved() {}\n"}}],
                    "edges":[{"from":"research","to":"read"},{"from":"read","to":"edit"}],
                    "after":[{"kind":"file_contains","path":path,
                        "text":"fn improved()"}]}});
            let op_ids=json!(["op-search","op-read","op-edit"]);
            let method_op=|id:&str,node:&str,tool:&str,arguments:Value,result:Value|
                json!({"id":id,"status":"completed","origin":"procedure",
                    "procedureStrategyId":"accepted-method",
                    "procedureRevision":"parent-v1","procedureNodeId":node,
                    "tool":tool,"arguments":arguments,"result":result});
            let search=method_op("op-search","research",investigation,
                json!({}),json!({"matches":[{"path":path}]}));
            let read=method_op("op-read","read","workspace_read_range",
                json!({"path":path}),json!({"sourceSha256":"a".repeat(64)}));
            let edit=method_op("op-edit","edit","workspace_replace",
                json!({"path":path,"expectedSha256":"a".repeat(64)}),
                json!({"written":true,"postSha256":"b".repeat(64)}));
            let ops=vec![search.clone(),read.clone(),edit.clone()];
            let receipt=json!({"methodId":"accepted-method","revision":"parent-v1",
                "executionId":"accepted-execution","operationIds":op_ids,
                "result":{"verified":true,"followed":true,"status":"followed"}});
            state.store.put("rsi_candidate",&json!({"id":"accepted-candidate",
                "status":"verified","paths":[path],
                "evaluation":{"metrics":{"eligible":true}}})).unwrap();
            let mut accepted=serde_json::Map::new();
            accepted.insert(path.to_string(),json!("b".repeat(64)));
            let generation=json!({"jobId":"gen1","accepted":true,
                "candidateId":"accepted-candidate",
                "acceptedSourceHashes":Value::Object(accepted)});
            let proposed=synthesize_observed_dag_pruning(
                &state,&method,&receipt,&ops,&generation).unwrap();
            assert_eq!(proposed["synthesis"]["kind"],"successful_observation_pruning");
            assert_eq!(proposed["status"],"candidate");
            assert_eq!(proposed["successfulApplications"],0);
            assert_eq!(proposed["synthesis"]["qualityGain"],Value::Null);
            assert_eq!(proposed["synthesis"]["prunedNodeId"],"research");
            assert_eq!(proposed["verification"]["nodes"].as_array().unwrap().len(),2);
            assert_eq!(proposed["verification"]["edges"],
                json!([{"from":"read","to":"edit"}]));
            let bindings=std::collections::HashMap::new();
            let (first,tool,_)=crate::engine::procedure::ready_intent(
                &proposed,&[],"improve",&bindings).unwrap();
            assert_eq!(first,"read");
            assert_eq!(tool,"workspace_read_range");
            let mut bindings=bindings;
            bindings.insert("read".to_string(),json!({"sourceSha256":"fresh-hash"}));
            let done=json!({"id":"new-read","origin":"procedure","status":"completed",
                "procedureStrategyId":proposed["id"],
                "procedureRevision":proposed["revision"],"procedureNodeId":"read",
                "result":{"sourceSha256":"fresh-hash"}});
            let (_,tool,args)=crate::engine::procedure::ready_intent(
                &proposed,&[done],"improve",&bindings).unwrap();
            assert_eq!(tool,"workspace_replace");
            assert_eq!(args["expectedSha256"],"fresh-hash");
            let rejected=json!({"jobId":"gen1","accepted":false});
            assert!(synthesize_observed_dag_pruning(
                &state,&method,&receipt,&ops,&rejected).is_none());
            let mut necessary=method.clone();
            necessary["verification"]["nodes"][1]["arguments"]["dependsOn"]=
                json!("$research.matches");
            assert!(synthesize_observed_dag_pruning(
                &state,&necessary,&receipt,&ops,&generation).is_none(),
                "a consumed exploration result cannot be removed");
            necessary=method.clone();
            necessary["verification"]["after"].as_array_mut().unwrap()
                .push(json!({"kind":"tool_called","tool":investigation}));
            assert!(synthesize_observed_dag_pruning(
                &state,&necessary,&receipt,&ops,&generation).is_none(),
                "a required tool observation cannot be pruned");
            assert!(synthesize_observed_dag_pruning(
                &state,&method,&receipt,&[read,edit],&generation).is_none(),
                "never invent a completed observation that did not occur");
            let _=std::fs::remove_dir_all(root);
        }
    }

    #[test]
    fn adopted_meta_operator_transfers_across_domains_without_earning_target_credit() {
        let (state,root)=state();
        let source=root.join("source");
        let input=json!({"rsiSourceDir":source,"rsiObjective":"tools improve"});
        state.store.put("job",&json!({"id":"tools-generation",
            "kind":"rsi_generation","status":"running",
            "input":input})).unwrap();
        let donor=json!({"id":"accepted-pruner","revision":"memory-v2",
            "domain":"memory","status":"executable",
            "successfulApplications":2,
            "applications":[{"accepted":true,"postconditionsPassed":true}],
            "synthesis":{"kind":"successful_observation_pruning",
                "transferDepth":0}});
        state.store.put("experience",&json!({"id":"donor-exp",
            "status":"active","partitionKey":research_scope(&input).partition_key,
            "executableMethods":[donor]})).unwrap();
        let path="src/engine/tools.rs";
        let target=json!({"id":"tools-method","revision":"tools-v1",
            "domain":"tools","name":"tool search then source edit",
            "assistanceMode":"harness_assisted",
            "verification":{"type":"procedure",
                "appliesTo":{"requestContains":["tools"]},
                "nodes":[
                    {"id":"search","tool":"code_search",
                        "arguments":{"query":"tools source"}},
                    {"id":"edit","tool":"workspace_replace",
                        "arguments":{"path":path,"startLine":1,"endLine":1,
                            "newText":"fn source() {}","expectedSha256":"a".repeat(64)}}],
                "edges":[{"from":"search","to":"edit"}],
                "after":[{"kind":"file_contains","path":path,"text":"fn source()"}]}});
        let receipts=json!({"methodId":"tools-method","revision":"tools-v1",
            "executionId":"tools-execution","operationIds":["op-search","op-write"],
            "result":{"verified":true,"followed":true,"status":"followed"}});
        let op=|id:&str,node:&str,tool:&str,result:Value|json!({
            "id":id,"origin":"procedure","status":"completed",
            "procedureStrategyId":"tools-method","procedureRevision":"tools-v1",
            "procedureNodeId":node,"tool":tool,
            "arguments":{"path":path},"result":result});
        let calls=vec![
            op("op-search","search","code_search",json!({"matches":["tools"]})),
            op("op-write","edit","workspace_replace",json!({"written":true}))
        ];
        let generation=json!({"jobId":"tools-generation","accepted":false});
        let evolved=synthesize_observed_dag_pruning(
            &state,&target,&receipts,&calls,&generation).unwrap();
        assert_eq!(evolved["synthesis"]["kind"],"transferred_observation_pruning");
        assert_eq!(evolved["synthesis"]["transferDonor"]["domain"],"memory");
        assert_eq!(evolved["synthesis"]["transferDepth"],1);
        assert_eq!(evolved["status"],"candidate");
        assert_eq!(evolved["successfulApplications"],0);
        assert_eq!(evolved["synthesis"]["qualityGain"],Value::Null);
        assert_eq!(evolved["verification"]["nodes"].as_array().unwrap().len(),1);
        state.store.put("experience",&json!({"id":"tools-exp",
            "status":"active","partitionKey":research_scope(&input).partition_key,
            "executableMethods":[target.clone(),evolved.clone()]})).unwrap();
        let decision=method_reuse_decision(&state,&input,"tools");
        assert_eq!(decision["repairMethodId"],evolved["id"]);
        assert_eq!(decision["repairSelectionEvidence"]["selection"],
            "success_ablation_trial");
        // The new domain's success may later teach a third domain only if
        // it independently gets an accepted candidate and a method receipt.
        let mut donor=state.store.get("experience","donor-exp").unwrap();
        donor["partitionKey"]=json!("rsi:other-repository");
        state.store.put("experience",&donor).unwrap();
        assert!(synthesize_observed_dag_pruning(
            &state,&target,&receipts,&calls,&generation).is_none());
        donor["partitionKey"]=json!(research_scope(&input).partition_key);
        donor["executableMethods"][0]["successfulApplications"]=json!(0);
        state.store.put("experience",&donor).unwrap();
        assert!(synthesize_observed_dag_pruning(
            &state,&target,&receipts,&calls,&generation).is_none());
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn method_scheduler_learns_evidence_weighted_utility_not_lifetime_popularity() {
        let (state,root)=state();
        let input=json!({"rsiSourceDir":root.join("src").to_string_lossy(),
            "rsiObjective":"memory improve"});
        let method=|id:&str,kind:&str,wins:usize,trials:usize|{
            let mut applications=Vec::new();
            for i in 0..trials {
                let accepted=i<wins;
                applications.push(json!({"accepted":accepted,
                    "postconditionsPassed":accepted,
                    "methodWroteAcceptedCandidate":accepted}));
            }
            json!({"id":id,"revision":id,"name":id,
                "domain":"memory","status":"executable",
                "assistanceMode":"harness_assisted",
                "successfulApplications":wins,
                "applications":applications,
                "synthesis":{"kind":kind},
                "verification":{"type":"procedure",
                    "appliesTo":{"requestContains":["memory"]},
                    "nodes":[{"id":"edit","tool":"workspace_replace",
                        "arguments":{"path":"src/engine/memory.rs",
                            "startLine":1,"endLine":1,
                            "newText":"fn improved() {}","expectedSha256":null}}],
                    "after":[{"kind":"file_contains",
                        "path":"src/engine/memory.rs","text":"improved"}]}})
        };
        let historical=method("historical","stale_source_rebase",6,20);
        let efficient=method("efficient","indexed_source_rebase",2,2);
        let (observations,coefficient)=method_selection_feedback(
            &[historical.clone(),efficient.clone()]);
        assert_eq!(observations,22);
        assert!(coefficient>0.20 && coefficient<0.45);
        assert!(method_observed_selection_score(
            &efficient,observations,coefficient)
                >method_observed_selection_score(
                    &historical,observations,coefficient));
        state.store.put("experience",&json!({"id":"observed-methods",
            "status":"active","partitionKey":research_scope(&input).partition_key,
            "executableMethods":[historical,efficient]})).unwrap();
        let selected=method_reuse_decision(&state,&input,"memory");
        assert_eq!(selected["repairMethodId"],"efficient",
            "fewer but more reliable adopted changes must outrank raw usage counts");
        assert_eq!(selected["methodIds"].as_array().unwrap().len(),1,
            "alternative source-edit programs may not mutate one path together");
        assert_eq!(selected["repairSelectionEvidence"]["selectionPolicy"],
            "observed_method_outcomes_posterior_v1");
        assert_eq!(selected["repairSelectionEvidence"]["selectionPolicyTrials"],22);
        let (n_old,e_old)=method_selection_feedback(
            &[method("a","stale_source_rebase",8,8)]);
        let (n_bad,e_bad)=method_selection_feedback(
            &[method("a","stale_source_rebase",8,20)]);
        assert_eq!(n_old,8);
        assert_eq!(n_bad,20);
        assert!(e_bad>e_old,
            "after more observed failures the next scheduler explores more");
        let _=std::fs::remove_dir_all(root);
    }

