    #[tokio::test]
    async fn goal_capability_loop_inherits_research_adopts_and_applies_next_decision() {
        let (state, root) = state();
        let context = parent(&state);
        let source = root.join("source");
        std::fs::create_dir_all(source.join("src")).unwrap();
        std::fs::write(source.join("Cargo.toml"),"[package]\nname=\"dominion-rust\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[dependencies]\nserde_json=\"1\"\nsha2=\"0.10\"\nrusqlite={version=\"0.32\",features=[\"bundled\"]}\n").unwrap();
        std::fs::write(source.join(".gitignore"), "/target\n").unwrap();
        std::fs::write(source.join("src/main.rs"), include_str!("../../../../tests/fixtures/rsi-generation-worker.rs")).unwrap();
        std::fs::write(source.join("build.rs"),r#"fn main(){let out=std::process::Command::new("git").args(["rev-parse","HEAD"]).output().unwrap();println!("cargo:rustc-env=FIXTURE_COMMIT={}",String::from_utf8_lossy(&out.stdout).trim());println!("cargo:rerun-if-env-changed=DOMINION_RSI_BUILD_COMMIT");}"#).unwrap();
        let mut lock = Command::new("cargo");
        lock.current_dir(&source).args(["generate-lockfile", "--offline"]);
        let output = command(&context, lock, Duration::from_secs(30)).await.unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        git(&context, &source, &["init", "-q"]).await.unwrap();
        git(&context, &source, &["add", "."]).await.unwrap();
        git(&context, &source, &["-c","user.name=Test","-c","user.email=test@localhost","commit","-qm","controlled worker"]).await.unwrap();
        let base = git(&context, &source, &["rev-parse", "HEAD"]).await.unwrap();
        let cases=["zero_boundary","negative_one","negative_two","negative_three","positive_one","positive_two","positive_three","positive_four"]
            .into_iter().enumerate().map(|(i,name)|json!({"id":name,"domain":if i%2==0{"boundary"}else{"retention"},"holdout":i<2,"filter":format!("cases::{name}")})).collect::<Vec<_>>();
        context.checkpoint(json!({"input":{"rsiNativeApproved":true,"rsiSourceDir":source,"rsiBaseCommit":base,
            "selectionPolicy":"automatic","rsiFrozenCases":cases,
            "rsiObjective":"memory capability improvement",
            "rsiFocusFiles":["src/engine/memory.rs"],
            "rsiFixtureMode":"research_then_adopt",
            "limits":{"maxAttempts":4,"tokenBudget":1000,"timeBudgetSeconds":300}}})).unwrap();
        let job = state.store.get("job", "campaign").unwrap();
        let result = tokio::time::timeout(Duration::from_secs(300), run(&context, &job)).await.unwrap().unwrap();
        let evolution = &result["evolution"];
        assert_eq!(result["status"], "completed", "{result}");
        assert_eq!(evolution["attempts"], 4, "{result}");
        assert_eq!(evolution["acceptedGenerations"], 1, "{result}");
        let history = evolution["history"].as_array().unwrap();
        assert_eq!(history.len(), 4);
        assert!(history[0]["candidateId"].is_null());
        assert!(history[0]["newFindings"].as_u64().unwrap() > 0);
        assert!(history[0]["knowledgeId"].as_str().is_some());
        assert_eq!(history[1]["researchDelivered"], true, "next generation did not receive stored research");
        assert_eq!(history[1]["accepted"], true, "{result}");
        assert_eq!(history[1]["decisionInput"]["nextDomain"], "memory",
            "the user goal did not drive the next capability selection");
        let accepted_commit = history[1]["acceptedCommit"].as_str().unwrap();
        assert_eq!(history[2]["generatorCommit"], accepted_commit);
        assert_ne!(history[2]["generatorBinarySha256"], history[1]["generatorBinarySha256"]);
        assert_eq!(history[2]["workerMarker"], "improved_execution");
        assert_eq!(history[2]["boundaryResult"], false, "the rebuilt function must actually execute");
        assert!(history[2]["candidateId"].is_string());
        assert_eq!(history[2]["accepted"], false, "a comment-only candidate must not be adopted");
        assert!(history[2]["newFindings"].as_u64().unwrap() > 0);
        assert!(history[2]["knowledgeId"].as_str().is_some());
        assert_ne!(history[2]["knowledgeId"], history[0]["knowledgeId"]);
        let inherited = &history[2]["inheritedImplementations"][0];
        assert_eq!(inherited["status"], "available_in_binary");
        assert_eq!(inherited["implementation_ref"]["commit"], accepted_commit);
        assert_eq!(history[2]["decisionInput"]["basis"], "accepted_implementation");
        assert_eq!(history[3]["decisionInput"]["basis"], "research_findings");
        assert_eq!(history[3]["decisionInput"]["observed"]["accepted"], false);
        assert!(history[3]["decisionInput"]["observed"]["tokens"].is_number());
        assert!(history[3]["decisionInput"]["observed"]["elapsedMs"].is_number());
        assert_eq!(history[3]["domain"], history[3]["decisionInput"]["nextDomain"]);
        assert_eq!(history[3]["searchStrategy"]["decisionSource"], "parent_next_decision",
            "the fourth generation did not execute the parent's next decision");
        assert_eq!(history[3]["workerMarker"], "decision_use");
        assert_eq!(evolution["nextDecision"]["basis"], "no_progress");
        assert_eq!(evolution["nextDecision"]["fromGeneration"], 4);
        assert_eq!(
            state.store.get("rsi_current", "selected-engine").unwrap()["commit"],
            accepted_commit
        );
        assert_eq!(git(&context, &source, &["rev-parse", "HEAD"]).await.unwrap(), base);
        for row in history {
            let stored=state.store.get("rsi_generation",row["id"].as_str().unwrap()).unwrap();
            for field in ["newFindings","knowledgeId","knowledgeArtifactHashes","methodExecutions","accepted"] {
                assert_eq!(stored[field],row[field],"next workers must read the completed {field}");
            }
        }
        // The same accepted chain can reach the real test branch through an
        // explicit product action. Untracked research and main are preserved.
        git(&context,&source,&["update-ref","refs/heads/main",&base]).await.unwrap();
        crate::engine::dev::save_config(&state,&json!({"sourceDir":source}));
        assert!(rsi_worktree::apply_campaign_to_sub(&state,"campaign",accepted_commit).await.unwrap_err().contains("dominion-sub"),"main must never be an application target");
        git(&context,&source,&["checkout","-b","dominion-sub2"]).await.unwrap();
        std::fs::write(source.join("my-research.txt"),"keep my unrelated research").unwrap();
        crate::engine::dev::save_config(&state,&json!({"sourceDir":source}));
        context.checkpoint(json!({"status":"completed"})).unwrap();
        let original=state.store.all("rsi_candidate").into_iter()
            .find(|c|c["experimentAdoption"]["commit"]==accepted_commit).unwrap();
        let mut tampered=original.clone();tampered["patch"]=json!("unverified replacement");
        state.store.put("rsi_candidate",&tampered);
        assert!(rsi_worktree::apply_campaign_to_sub(&state,"campaign",accepted_commit).await.is_err());
        state.store.put("rsi_candidate",&original);
        let mut stopped=state.store.get("rsi_campaign","campaign").unwrap();
        stopped["status"]=json!("cancelled");stopped["acceptedGenerations"]=json!(0);
        state.store.put("rsi_campaign",&stopped);
        // A later stopped campaign may carry an earlier verified selection.
        // No new model generation is needed merely to publish that selection.
        let applied=rsi_worktree::apply_campaign_to_sub(&state,"campaign",accepted_commit).await.unwrap();
        assert_eq!(applied["commit"],accepted_commit);
        assert_eq!(applied["branch"],"dominion-sub2");
        assert!(rsi_worktree::apply_campaign_to_sub(&state,"campaign",accepted_commit).await.is_ok(),"retry must not apply twice");
        assert_eq!(std::fs::read_to_string(source.join("my-research.txt")).unwrap(),"keep my unrelated research");
        let rev=|reference:&str|String::from_utf8(std::process::Command::new("git").current_dir(&source)
            .args(["rev-parse",reference]).output().unwrap().stdout).unwrap().trim().to_string();
        assert_eq!(rev("HEAD"),accepted_commit);assert_eq!(rev("main"),base);
        context.checkpoint(json!({"status":"running"})).unwrap();
        let lab = PathBuf::from(evolution["labDir"].as_str().unwrap());
        git(&context, &source, &["worktree", "remove", "--force", lab.to_str().unwrap()]).await.unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    fn generation(domain: &str, accepted: bool) -> Value {
        json!({
            "domain": domain,
            "accepted": accepted,
            "tokens": 10_000,
            "candidateId": if accepted { json!("candidate") } else { Value::Null },
            "evaluation": {"verdict": if accepted { "improved" } else { "inconclusive" }}
        })
    }

    fn active_generation(
        job: &str,
        attempt: u64,
        domain: &str,
        accepted: bool,
        commit: &str,
        tokens: u64,
        elapsed: u64,
    ) -> Value {
        json!({
            "jobId": job, "attempt": attempt, "domain": domain, "accepted": accepted,
            "generatorCommit": commit, "domainSelection": "algorithm_active",
            "tokens": tokens, "elapsedMs": elapsed,
            "evaluation": {"verdict": if accepted { "improved" } else { "inconclusive" }}
        })
    }

    fn trial_generation(
        job: &str,
        attempt: u64,
        domain: &str,
        accepted: bool,
        commit: &str,
        tokens: u64,
        elapsed: u64,
        candidate: &str,
    ) -> Value {
        json!({
            "jobId": job, "attempt": attempt, "domain": domain, "accepted": accepted,
            "generatorCommit": commit, "domainSelection": "algorithm_trial",
            "searchStrategy": {"id": candidate, "mode": "trial"},
            "tokens": tokens, "elapsedMs": elapsed,
            "evaluation": {"verdict": if accepted { "improved" } else { "inconclusive" }}
        })
    }

    fn search_expression(name: &str, expression: &str) -> Value {
        json!({
            "id": format!("algorithm-{name}"),
            "kind": "selfimprove.search",
            "name": name,
            "variant": name,
            "implementation": "expression",
            "revision": format!("revision-{name}"),
            "parameters": {"expression": expression}
        })
    }

    #[test]
    fn empty_model_error_does_not_abort_a_campaign() {
        assert_eq!(generation_model_error(&json!({"result":{"rsi":{"modelError":""}}})), None);
        assert_eq!(generation_model_error(&json!({"result":{"rsi":{"modelError":"  "}}})), None);
        assert_eq!(generation_model_error(&json!({"result":{"rsi":{}}})), None);
        assert_eq!(
            generation_model_error(&json!({"result":{"rsi":{"modelError":"rsi_token_budget: stop"}}})),
            Some("rsi_token_budget: stop")
        );
    }

    #[tokio::test]
    async fn source_snapshot_fingerprint_tracks_the_commit() {
        let (state, root) = state();
        state.store.put("job", &json!({"id":"snapshot-test","status":"running","cancelRequested":false}));
        let source = root.join("source-repo");
        std::fs::create_dir_all(&source).unwrap();
        let run_git = |args: &[&str]| {
            std::process::Command::new("git")
                .current_dir(&source)
                .args(["-c","user.name=test","-c","user.email=test@localhost","-c","commit.gpgsign=false"])
                .args(args)
                .output()
                .unwrap()
        };
        assert!(run_git(&["init","-q"]).status.success());
        std::fs::write(source.join("value.txt"), "one").unwrap();
        assert!(run_git(&["add","."]).status.success());
        assert!(run_git(&["commit","-q","-m","one"]).status.success());
        let lab = root.join("lab");
        std::fs::create_dir_all(&lab).unwrap();
        std::process::Command::new("git").current_dir(&lab).args(["init","-q"]).output().unwrap();
        let context = crate::engine::jobs::JobContext::new(state.clone(), "snapshot-test".into());
        let (_, first) = snapshot_source(&context, &source, &lab).await.unwrap();
        std::fs::write(source.join("value.txt"), "two").unwrap();
        assert!(run_git(&["commit","-qam","two"]).status.success());
        let (_, second) = snapshot_source(&context, &source, &lab).await.unwrap();
        assert!(lineage_fingerprint_valid(&first), "a clean checkout must not hash to the empty digest");
        assert_ne!(first, second, "a new commit must change the lineage fingerprint");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn lineage_fingerprint_identifies_the_source_commit() {
        let empty_hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let first = lineage_fingerprint("a".repeat(40).as_str(), "b".repeat(40).as_str(), b"", &[]);
        let second = lineage_fingerprint("c".repeat(40).as_str(), "b".repeat(40).as_str(), b"", &[]);
        assert_eq!(first.len(), 64);
        assert_ne!(first, empty_hash, "a clean checkout still has a commit and tree");
        assert_ne!(first, second, "different commits must not share a fingerprint");
        assert!(lineage_fingerprint_valid(&first));
        assert!(!lineage_fingerprint_valid(empty_hash));
        assert!(!lineage_fingerprint_valid(""));
    }

    #[test]
    fn korean_goals_select_the_requested_capability_without_focus_files() {
        let algorithm=json!({"implementation":"failure-clusters"});
        for (goal,domain) in [
            ("오래된 사실을 기억하고 회상해","memory"),
            ("대화 토큰 비용을 줄여","context"),
            ("외부 지식을 학습해","learning"),
            ("브라우저 검색 도구를 연결해","tools"),
            ("버튼과 화면을 단순하게 해","ui"),
            ("자기개선 알고리즘을 발전시켜","improver"),
        ] {assert_eq!(select_domain(&algorithm,&[],goal),domain,"{goal}");}
        assert!(!objective_matches("ui","build the service"));
    }

    #[tokio::test]
    async fn learned_method_uses_real_tools_and_failed_postconditions_change_reuse() {
        use axum::{routing::post,Json,Router,response::IntoResponse};
        // This local provider only substitutes model decisions. The production
        // runtime must read the research, register a DAG, bind results, execute
        // tools, verify effects and update the following decision itself.
        async fn provider(Json(body):Json<Value>)->axum::response::Response {
            let messages=body["messages"].as_array().unwrap();
            let last=messages.last().unwrap();
            let request=messages.iter().rev().find(|m|m["role"]=="user")
                .and_then(|m|m["content"].as_str()).unwrap_or("");
            let mut call=None;
            if request.contains("학습 조사") {
                if last["role"]=="tool" {
                    let result:Value=serde_json::from_str(last["content"].as_str().unwrap()).unwrap();
                    if let Some(content)=result["content"].as_str() {
                        let (_,source)=content.trim_start().split_once(' ').unwrap();
                        let knowledge:Value=serde_json::from_str(source.trim()).unwrap();
                        call=Some(("rsi_method_propose",json!({"name":"연구에서 합성한 자료 처리","domain":"context",
                            "description":knowledge["principle"],
                            "verification":{"type":"procedure","appliesTo":{"requestContains":["대화"]},
                                "nodes":[
                                    {"id":"source","tool":"workspace_read_range","arguments":{"path":knowledge["input"],"startLine":1,"endLine":2}},
                                    {"id":"derive","tool":"workspace_write","arguments":{"path":knowledge["output"],"content":"$source.content"}}
                                ],"edges":[{"from":"source","to":"derive"}],
                                "after":[{"kind":"file_contains","path":knowledge["output"],"text":knowledge["needle"]}]}})));
                    }
                } else {
                    call=Some(("workspace_read_range",json!({"path":"src/recipe.json","startLine":1,"endLine":1})));
                }
            }
            let (delta,finish)=if let Some((name,args))=call {
                (json!({"tool_calls":[{"index":0,"id":format!("call-{name}"),"type":"function",
                    "function":{"name":name,"arguments":args.to_string()}}]}),"tool_calls")
            }else{(json!({"content":"작업 완료"}),"stop")};
            let events=format!("data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                json!({"choices":[{"delta":delta}]}),
                json!({"choices":[{"delta":{},"finish_reason":finish}],"usage":{"prompt_tokens":10,"completion_tokens":10,"total_tokens":20}}));
            ([(axum::http::header::CONTENT_TYPE,"text/event-stream")],events).into_response()
        }
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let server=tokio::spawn(async move {axum::serve(listener,Router::new().route("/chat/completions",post(provider))).await.unwrap()});
        let (state,root)=state();
        let _parent=parent(&state);
        std::fs::create_dir_all(state.config.workspace.join("src")).unwrap();
        let marker=format!("learned-{}",uuid::Uuid::new_v4());
        std::fs::write(state.config.workspace.join("src/input.txt"),&marker).unwrap();
        std::fs::write(state.config.workspace.join("src/recipe.json"),json!({
            "input":"src/input.txt","output":"src/derived.txt","needle":marker,
            "principle":"읽은 결과를 다음 도구의 입력으로 연결한다"}).to_string()).unwrap();
        state.store.put("provider",&json!({"id":"local-test","kind":"openai","baseUrl":format!("http://{address}"),
            "model":"test","toolMode":"native","enabled":true}));
        let input=json!({"rsiObjective":"대화 자료 처리 능력","rsiDomain":"context",
            "rsiKnowledgeSource":state.config.workspace,"rsiSourceDir":state.config.workspace});
        for (index,session_id) in ["learn","use","counterexample","after-failure"].into_iter().enumerate() {
            state.store.put("job",&json!({"id":session_id,"kind":"rsi_generation","status":"running",
                "rsiWorkspace":state.config.workspace,"input":{"rsiCampaignId":"campaign"}}));
            let mut session=json!({"id":session_id,"providerId":"local-test","rsiRun":true,
                "rsiObjective":input["rsiObjective"],"rsiCampaignId":"campaign","rsiGenerationJobId":session_id});
            configure_method_session(&state,&input,&mut session);
            state.store.put("session",&session);
            if index==2 {std::fs::write(state.config.workspace.join("src/input.txt"),"changed premise").unwrap();}
            let request=crate::engine::chat::TurnRequest::from_value(&json!({"sessionId":session_id,
                "input":if index==0{"대화 방법을 학습 조사해"}else{"대화 자료를 처리해"},
                "toolsEnabled":true,"toolsExecutionEnabled":true,"autoLearning":false})).unwrap();
            tokio::time::timeout(Duration::from_secs(10),crate::engine::run_quiet(&state,&request))
                .await.unwrap().unwrap();
            collect_method_executions(&state,session_id,&state.config.workspace);
            let generation=json!({"jobId":session_id,"accepted":false,"domain":"context","newFindings":1,"tokens":20,"elapsedMs":1});
            if index==0 {
                let id=remember_research(&state,&input,&generation,session_id).expect("research and executable proposal must persist");
                let record=state.store.get("experience",&id).unwrap();
                assert_eq!(record["executableMethods"].as_array().unwrap().len(),1);
                assert!(!record["researchArtifacts"].as_array().unwrap().is_empty());
                assert_eq!(method_reuse_decision(&state,&input,"context")["action"],"execute_methods");
            }else {
                let receipts=record_method_feedback(&state,&generation,session_id);
                if index<3 {
                    assert_eq!(receipts.len(),1,"actual engine procedure invocation missing");
                    assert_eq!(receipts[0]["operationIds"].as_array().unwrap().len(),2);
                    assert_eq!(receipts[0]["result"]["followed"],index==1,"{receipts:?}");
                }else{assert!(receipts.is_empty(),"failed method must not run again");}
                if index==1 {
                    assert!(std::fs::read_to_string(state.config.workspace.join("src/derived.txt")).unwrap().contains(&marker));
                    assert_eq!(method_reuse_decision(&state,&input,"context")["action"],"execute_methods");
                    // A different repository and an explicit no-reuse decision cannot inject it.
                    let mut probe=session.clone();probe["id"]=json!("not-yet-executed");
                    assert_eq!(method_strategies(&state,&probe).len(),1);
                    let mut foreign=probe.clone();foreign["rsiResearchPartition"]=json!("other");
                    assert!(method_strategies(&state,&foreign).is_empty());
                    foreign=probe.clone();foreign["rsiDecisionInput"]["reuseDecision"]["action"]=json!("research_only");
                    assert!(method_strategies(&state,&foreign).is_empty());
                }else{assert_eq!(method_reuse_decision(&state,&input,"context")["action"],"research_only");}
            }
        }
        server.abort();
        let _=std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn reconstructed_existing_file_repair_runs_through_actual_native_tools() {
        use axum::{routing::post,Json,Router,response::IntoResponse};
        async fn provider(Json(_body):Json<Value>)->axum::response::Response {
            let events=format!("data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                json!({"choices":[{"delta":{"content":"완료"}}]}),
                json!({"choices":[{"delta":{},"finish_reason":"stop"}],
                    "usage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12}}));
            ([(axum::http::header::CONTENT_TYPE,"text/event-stream")],events).into_response()
        }
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let server=tokio::spawn(async move {
            axum::serve(listener,Router::new().route("/chat/completions",post(provider)))
                .await.unwrap()
        });
        let (state,root)=state();
        let _parent=parent(&state);
        let target="src/engine/memory.rs";
        std::fs::create_dir_all(state.config.workspace.join("src/engine")).unwrap();
        std::fs::write(state.config.workspace.join(target),"fn old() {}\n").unwrap();
        state.store.put("provider",&json!({"id":"repair-provider","kind":"openai",
            "baseUrl":format!("http://{address}"),"model":"test",
            "toolMode":"native","enabled":true}));
        let original=json!({"id":"create","revision":"old","name":"repair file",
            "domain":"memory","assistanceMode":"harness_assisted",
            "verification":{"type":"procedure","appliesTo":{"requestContains":["memory"]},
                "nodes":[{"id":"write","tool":"workspace_write","arguments":{
                    "path":target,"content":"fn improved() {}\n"}}],
                "after":[{"kind":"file_equals","path":target,
                    "text":"fn improved() {}\n"}]}});
        let failed=json!({"id":"failed-create","origin":"procedure","status":"failed",
            "tool":"workspace_write","procedureStrategyId":"create",
            "procedureRevision":"old","procedureNodeId":"write",
            "arguments":{"path":target,"content":"fn improved() {}\n"},
            "result":{"error":"RSI의 workspace_write는 새 파일 생성 전용입니다."}});
        let repaired=synthesize_existing_file_write_repair(&original,&failed).unwrap();
        let input=json!({"rsiObjective":"memory improvement","rsiDomain":"memory",
            "rsiKnowledgeSource":state.config.workspace,"rsiSourceDir":state.config.workspace});
        state.store.put("experience",&json!({"id":"repaired-experience",
            "status":"active","partitionKey":research_scope(&input).partition_key,
            "executableMethods":[repaired.clone()]})).unwrap();
        state.store.put("job",&json!({"id":"repair-child","kind":"rsi_generation",
            "status":"running","rsiWorkspace":state.config.workspace,
            "input":{"rsiCampaignId":"campaign"}})).unwrap();
        let mut session=json!({"id":"repair-session","providerId":"repair-provider",
            "rsiRun":true,"rsiObjective":"memory improvement",
            "rsiCampaignId":"campaign","rsiGenerationJobId":"repair-child"});
        configure_method_session(&state,&input,&mut session);
        assert_eq!(method_strategies(&state,&session).len(),1);
        state.store.put("session",&session).unwrap();
        let request=crate::engine::chat::TurnRequest::from_value(&json!({
            "sessionId":"repair-session","input":"memory improvement",
            "toolsEnabled":true,"toolsExecutionEnabled":true,"autoLearning":false
        })).unwrap();
        tokio::time::timeout(Duration::from_secs(10),
            crate::engine::run_quiet(&state,&request)).await.unwrap().unwrap();
        assert_eq!(std::fs::read_to_string(
            state.config.workspace.join(target)).unwrap(),"fn improved() {}\n");
        let operations=state.store.all("execution_operation").into_iter()
            .filter(|op|op["sessionId"]=="repair-session"
                && op["origin"]=="procedure")
            .collect::<Vec<_>>();
        assert!(operations.iter().any(|op|
            op["tool"]=="workspace_read_range" && op["status"]=="completed"));
        assert!(operations.iter().any(|op|
            op["tool"]=="workspace_replace" && op["status"]=="completed"
                && op["result"]["written"]==true));
        collect_method_executions(&state,"repair-session",&state.config.workspace);
        let feedback=record_method_feedback(&state,
            &json!({"jobId":"repair-child","accepted":false,
                "newFindings":1,"tokens":12,"elapsedMs":10}),"repair-session");
        assert_eq!(feedback.len(),1);
        assert_eq!(feedback[0]["result"]["followed"],true);
        assert_eq!(state.store.get("experience","repaired-experience").unwrap()
            ["executableMethods"][0]["successfulApplications"],json!(0),
            "successful tool execution without independently verified source adoption is not quality gain");
        server.abort();
        let _=std::fs::remove_dir_all(root);
    }

