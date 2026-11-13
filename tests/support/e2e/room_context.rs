#[test]
fn common_participant_state_follows_provider_and_protocol_changes() {
    let _serial=serial();
    let (port,requests)=scripted_provider(|body| {
        if body["input"].is_array() {
            vec![json!({"type":"response.output_text.delta","delta":"COMMON_LOGICAL_42"}),json!({"type":"response.completed","response":{"usage":{"input_tokens":100,"output_tokens":5,"total_tokens":105}}})]
        } else if !body["system"].is_null() {
            vec![json!({"type":"message_start","message":{"usage":{"input_tokens":100,"output_tokens":0}}}),json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"COMMON_LOGICAL_42"}}),json!({"type":"message_delta","usage":{"output_tokens":5}}),json!({"type":"message_stop"})]
        } else {
            vec![json!({"choices":[{"delta":{"content":"COMMON_LOGICAL_42"},"finish_reason":"stop"}],"usage":{"prompt_tokens":100,"completion_tokens":5,"total_tokens":105}})]
        }
    });
    let app=TestApp::new(port,"native");
    let agent=app.post("/api/agents",&json!({"name":"portable participant","systemPrompt":"SHARED_IDENTITY_MUST_STAY","providerId":"","model":""}));
    let session=app.post("/api/sessions",&json!({"title":"same logical conversation","agentId":agent["id"]}));
    let mut participant=Value::Null;
    let mut previous_reply=Value::Null;
    for (index,kind) in ["openai","anthropic","codex","ollama"].into_iter().enumerate() {
        let provider=app.post("/api/providers",&json!({"name":format!("{kind} protocol fixture"),"kind":kind,"local":kind=="ollama","baseUrl":format!("http://127.0.0.1:{port}/v1"),"enabled":true,"model":format!("route-{index}"),"accountId":"fixture-account","apiKey":"fixture-only","toolMode":"native"}));
        let stream=app.chat(&json!({"sessionId":session["id"],"agentId":agent["id"],"providerId":provider["id"],"model":provider["model"],"input":if index==0{"식별자를 말해라"}else{"앞서 말한 식별자는?"},"toolsEnabled":false,"autoLearning":false}));
        assert!(stream.contains("COMMON_LOGICAL_42"),"{kind}: {stream}");
        let messages=app.get(&format!("/api/sessions/{}/messages",session["id"].as_str().unwrap()));
        let last=messages.as_array().unwrap().iter().rev().find(|row|row["role"]=="assistant").unwrap();
        let continuity=&last["metadata"]["continuity"];
        if index==0 {participant=continuity["participantId"].clone();}
        assert_eq!(continuity["participantId"],participant,"{kind}");
        assert_eq!(continuity["previousReplyMessageId"],previous_reply,"{kind}");
        previous_reply=last["id"].clone();
        assert!(last["metadata"]["usage"]["cachedPromptTokens"].is_null(),"missing cache telemetry is unknown: {last}");
        let seen=requests.lock().unwrap();
        assert_eq!(seen.len(),index+1);
        let body=seen.last().unwrap();
        assert!(body.to_string().contains("SHARED_IDENTITY_MUST_STAY"));
        if index>0 {assert!(body.to_string().contains("COMMON_LOGICAL_42"));}
        if kind=="anthropic" {
            assert_eq!(body["system"][0]["cache_control"]["type"],"ephemeral");
        } else {assert!(body["prompt_cache_key"].is_string());}
    }
}

#[test]
fn strict_provider_cache_rejection_is_learned_and_keeps_the_same_conversation() {
    let _serial=serial();
    for kind in ["ollama","anthropic"] {
        let (port,requests)=scripted_provider(|body| {
            let field=if body["prompt_cache_key"].is_string(){Some("prompt_cache_key")}
                else if body["system"].as_array().is_some_and(|parts|parts.iter().any(|part|part.get("cache_control").is_some())){Some("cache_control")}else{None};
            if let Some(field)=field {
                return vec![json!({"_testHttpStatus":400,"body":{"error":{"param":field,"message":format!("Unsupported parameter: {field}")}}})];
            }
            if !body["system"].is_null() {
                vec![json!({"type":"message_start","message":{"usage":{"input_tokens":30}}}),json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"state kept"}}),json!({"type":"message_delta","usage":{"output_tokens":3}}),json!({"type":"message_stop"})]
            } else {
                vec![json!({"choices":[{"delta":{"content":"state kept"},"finish_reason":"stop"}],"usage":{"prompt_tokens":30,"completion_tokens":3,"total_tokens":33}})]
            }
        });
        let app=TestApp::new(port,"native");
        let provider=app.post("/api/providers",&json!({"name":"strict wire fixture","kind":kind,"local":kind=="ollama","baseUrl":format!("http://127.0.0.1:{port}/v1"),"enabled":true,"model":"strict-model","toolMode":"native"}));
        let agent=app.post("/api/agents",&json!({"name":"strict participant","systemPrompt":"FULL_IDENTITY","providerId":provider["id"],"model":"strict-model"}));
        let session=app.post("/api/sessions",&json!({"title":"strict provider","agentId":agent["id"]}));
        let mut participant=Value::Null;
        for index in 0..2 {
            let stream=app.chat(&json!({"sessionId":session["id"],"agentId":agent["id"],"input":"안녕","toolsEnabled":false,"autoLearning":false}));
            assert!(stream.contains("state kept"),"{kind}: {stream}");
            let rows=app.get(&format!("/api/sessions/{}/messages",session["id"].as_str().unwrap()));
            let last=rows.as_array().unwrap().iter().rev().find(|row|row["role"]=="assistant").unwrap();
            let continuity=&last["metadata"]["continuity"];
            if index==0 {participant=continuity["participantId"].clone();}
            assert_eq!(continuity["participantId"],participant);
            assert_eq!(continuity["cacheHintAcceptance"],false);
            assert_eq!(continuity["httpRequests"],if index==0{2}else{1});
            assert!(last["metadata"]["usage"]["cachedPromptTokens"].is_null());
            let seen=requests.lock().unwrap();
            assert_eq!(seen.len(),if index==0{2}else{3});
            assert!(seen.last().unwrap().to_string().contains("FULL_IDENTITY"));
        }
    }
}

#[test]
fn room_bot_continuity_is_distinct_and_survives_an_engine_restart() {
    let _serial=serial();
    let (port,requests)=scripted_provider(|_|vec![
        json!({"type":"response.output_text.delta","delta":"MY_BOT_PREVIOUS_ANSWER_42"}),
        json!({"type":"response.completed","response":{"usage":{"input_tokens":200,"input_tokens_details":{"cached_tokens":128},"output_tokens":10,"total_tokens":210}}}),
    ]);
    let app=TestApp::new(port,"native");
    let provider=app.post("/api/providers",&json!({"name":"continuity","kind":"codex","baseUrl":format!("http://127.0.0.1:{port}/codex"),"accountId":"fixture-account","apiKey":"fixture-only","enabled":true,"model":"fixture-model"}));
    let a=app.post("/api/agents",&json!({"name":"A","systemPrompt":"A identity stays verbatim","providerId":provider["id"],"model":"fixture-model"}));
    let b=app.post("/api/agents",&json!({"name":"B","systemPrompt":"B identity stays verbatim","providerId":provider["id"],"model":"fixture-model"}));
    let room=app.post("/api/rooms",&json!({"name":"continuous bots","agentIds":[a["id"],b["id"]],"routingMode":"mentioned","rounds":1,"autoModels":false,"autoLearning":false,"toolsEnabled":false,"models":[{"providerId":provider["id"],"model":"fixture-model","reasoningEffort":"low"}]}));
    fn send(app:&TestApp,room:&Value,agent:&Value,input:&str) {
        let submitted=app.post(&format!("/api/rooms/{}/messages",room["id"].as_str().unwrap()),&json!({"input":input,"agentIds":[agent["id"]]}));
        let job=submitted["job"]["id"].as_str().unwrap();
        let deadline=Instant::now()+Duration::from_secs(10);
        loop {
            let record=app.get(&format!("/api/jobs/{job}"));
            if record["status"]=="completed" {break;}
            assert_ne!(record["status"],"failed","{record}");
            assert!(Instant::now()<deadline,"{record}");
            std::thread::sleep(Duration::from_millis(40));
        }
    }
    send(&app,&room,&a,"처음 선택한 식별자를 말해라.");
    send(&app,&room,&b,"동료도 자기 식별자를 말해라.");
    let root=app._root.clone();
    drop(app);
    let app=TestApp::from_root(port,"native",root,&[]);
    send(&app,&room,&a,"앞서 네가 말했던 식별자는?");
    let seen=requests.lock().unwrap();
    assert_eq!(seen.len(),3,"no extra model for continuity");
    assert_eq!(seen[0]["prompt_cache_key"],seen[2]["prompt_cache_key"]);
    assert_ne!(seen[0]["prompt_cache_key"],seen[1]["prompt_cache_key"]);
    assert_eq!(seen[0]["instructions"],seen[2]["instructions"]);
    assert!(seen[0]["instructions"].as_str().unwrap().contains("A identity stays verbatim"));
    assert!(seen[1]["instructions"].as_str().unwrap().contains("B identity stays verbatim"));
    for request in seen.iter() {
        let headers=request["_testHttpHeaders"].as_str().unwrap().to_lowercase();
        assert!(headers.contains("session_id:"));
        assert!(headers.contains(&format!("x-client-request-id: {}",request["prompt_cache_key"].as_str().unwrap())));
        assert!(request.get("prompt_cache_options").is_none(),"consumer Codex owns cache policy");
    }
    assert!(seen[2]["input"].as_array().unwrap().iter().any(|row|row["role"]=="assistant" && row["content"][0]["text"]=="MY_BOT_PREVIOUS_ANSWER_42"));
    let messages=app.get(&format!("/api/rooms/{}/messages",room["id"].as_str().unwrap()));
    let rows=messages.as_array().unwrap();
    let last=rows.iter().rev().find(|row|row["role"]=="assistant").unwrap();
    assert_eq!(last["metadata"]["continuity"]["samePrefixAsPreviousRequest"],true);
    assert_eq!(last["metadata"]["usage"]["cachedPromptTokens"],128);
    assert_eq!(last["metadata"]["usage"]["uncachedPromptTokens"],72);
    let c=Connection::open(app._root.join("runtime/dominion.sqlite")).unwrap();
    let saved:String=c.query_row("SELECT data FROM records WHERE kind='model_conversation' AND id=?1",[last["metadata"]["continuity"]["id"].as_str().unwrap()],|row|row.get(0)).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&saved).unwrap()["lastReplyMessageId"],last["id"]);
}

#[test]
fn nine_room_participants_keep_actual_request_prefix_across_sliding_windows_and_restart() {
    let _serial=serial();
    let (port,requests)=scripted_provider(|_|vec![
        json!({"type":"response.output_text.delta","delta":"인사했습니다."}),
        json!({"type":"response.completed","response":{"usage":{"input_tokens":200,"output_tokens":5,"total_tokens":205}}}),
    ]);
    let mut app=TestApp::new(port,"native");
    let provider=app.post("/api/providers",&json!({"name":"prefix fixture","kind":"codex","baseUrl":format!("http://127.0.0.1:{port}/codex"),"accountId":"fixture-account","apiKey":"fixture-only","enabled":true,"model":"fixture-model"}));
    let agents=(0..9).map(|i|app.post("/api/agents",&json!({"name":format!("participant-{i}"),"systemPrompt":format!("VERBATIM_IDENTITY_{i}"),"providerId":provider["id"],"model":"fixture-model"}))).collect::<Vec<_>>();
    let room=app.post("/api/rooms",&json!({"name":"nine continuous participants","agentIds":agents.iter().map(|a|a["id"].clone()).collect::<Vec<_>>(),"routingMode":"discussion","rounds":1,"maxTurns":9,"autoModels":false,"autoLearning":false,"toolsEnabled":false,"models":[{"providerId":provider["id"],"model":"fixture-model","reasoningEffort":"low"}]}));
    for round in 0..3 {
        if round==2 {
            let root=app._root.clone();drop(app);
            app=TestApp::from_root(port,"native",root,&[]);
        }
        let submitted=app.post(&format!("/api/rooms/{}/messages",room["id"].as_str().unwrap()),&json!({"input":"다들 인사해라","agentIds":agents.iter().map(|a|a["id"].clone()).collect::<Vec<_>>()}));
        let deadline=Instant::now()+Duration::from_secs(20);
        loop {
            let job=app.get(&format!("/api/jobs/{}",submitted["job"]["id"].as_str().unwrap()));
            if job["status"]=="completed" {break;}
            assert_ne!(job["status"],"failed","{job}");
            assert!(Instant::now()<deadline,"{job}");
            std::thread::sleep(Duration::from_millis(30));
        }
        let seen=requests.lock().unwrap();
        assert_eq!(seen.len(),(round+1)*9,"exactly one model call per participant");
        if round>0 {
            for current in &seen[round*9..] {
                let previous=seen[(round-1)*9..round*9].iter().find(|old|old["prompt_cache_key"]==current["prompt_cache_key"]).unwrap();
                assert_eq!(current["instructions"],previous["instructions"]);
                let prefix=previous["input"].as_array().unwrap();
                let next=current["input"].as_array().unwrap();
                assert!(next.len()>prefix.len());
                assert_eq!(&next[..prefix.len()],prefix.as_slice(),"same key alone cannot prove request prefix reuse");
                assert!(!current.to_string().contains("_dominion_"));
            }
        }
    }
}

#[test]
fn codex_effort_survives_agent_bulk_chat_override_and_room_selection() {
    let _serial = serial();
    let (port, requests) = scripted_provider(|_| vec![
        json!({"type":"response.output_text.delta","delta":"확인했습니다."}),
        json!({"type":"response.completed","response":{"usage":{"input_tokens":30,"output_tokens":5,"total_tokens":35}}}),
    ]);
    let app = TestApp::new(port,"native");
    let provider = app.post("/api/providers", &json!({"name":"Codex test","kind":"codex","baseUrl":format!("http://127.0.0.1:{port}/codex"),"accountId":"test-account","apiKey":"test-only","enabled":true,"model":"catalog-model","reasoningEffort":"high"}));
    // Seed the normalized account catalog, without a real login or generation.
    let catalog = json!({"id":provider["id"],"accountId":"test-account","models":[{"id":"catalog-model","reasoningEfforts":["none","low","medium","high","xhigh","max","ultra"]}]});
    Connection::open(app._root.join("runtime/dominion.sqlite")).unwrap().execute(
        "INSERT INTO records(kind,id,data) VALUES('codex_model_catalog',?1,?2)",
        rusqlite::params![provider["id"].as_str().unwrap(),catalog.to_string()],
    ).unwrap();
    let agent = app.post("/api/agents", &json!({"name":"Effort agent","systemPrompt":"간결하게 응답하세요.","providerId":provider["id"],"model":"catalog-model","reasoningEffort":"ultra","thinking":false}));
    let chat = |effort: Option<&str>, expected:&str| {
        let session=app.post("/api/sessions",&json!({"title":"reasoning","agentId":agent["id"]}));
        let mut body=json!({"sessionId":session["id"],"agentId":agent["id"],"input":"안녕하세요","toolsEnabled":false,"autoLearning":false});
        if let Some(effort)=effort {body["reasoningEffort"]=json!(effort);}
        let before=requests.lock().unwrap().len();
        let stream=app.chat(&body);
        assert!(stream.contains("확인했습니다"),"{stream}");
        let seen=requests.lock().unwrap();
        assert_eq!(seen.len(),before+1,"no discovery/review calls: {seen:?}");
        assert_eq!(seen.last().unwrap()["reasoning"]["effort"],expected);
    };
    chat(None,"ultra");
    for effort in ["xhigh","max","ultra","none"] {chat(Some(effort),effort);}
    let off_session=app.post("/api/sessions",&json!({"title":"off","agentId":agent["id"]}));
    let off=app.chat(&json!({"sessionId":off_session["id"],"agentId":agent["id"],"input":"안녕하세요","thinking":false,"toolsEnabled":false,"autoLearning":false}));
    assert!(off.contains("확인했습니다"),"{off}");
    assert_eq!(requests.lock().unwrap().last().unwrap()["reasoning"]["effort"],"none");
    let bulk=app.post("/api/agents/bulk-model",&json!({"agentIds":[agent["id"]],"providerId":provider["id"],"model":"catalog-model","reasoningEffort":"max"}));
    assert_eq!(bulk["agents"][0]["reasoningEffort"],"max");
    chat(None,"max");
    let mut updated=bulk["agents"][0].clone();updated["reasoningEffort"]=json!("");
    app.post("/api/agents",&updated);
    chat(None,"high"); // Provider default is inherited only when agent/turn are unset.

    let room=app.post("/api/rooms",&json!({"name":"Effort room","agentIds":[agent["id"]],"routingMode":"mentioned","rounds":1,"autoModels":false,"autoLearning":false,"toolsEnabled":false,"reasoningEffort":"xhigh","models":[{"providerId":provider["id"],"model":"catalog-model","reasoningEffort":"ultra"}]}));
    let before=requests.lock().unwrap().len();
    let submitted=app.post(&format!("/api/rooms/{}/messages",room["id"].as_str().unwrap()),&json!({"input":"안녕하세요","agentIds":[agent["id"]]}));
    let job_id=submitted["job"]["id"].as_str().unwrap();
    let deadline=Instant::now()+Duration::from_secs(10);
    loop {
        let job=app.get(&format!("/api/jobs/{job_id}"));
        if job["status"]=="completed" {break;}
        assert!(job["status"]!="failed","{job}");
        assert!(Instant::now()<deadline,"{job}");
        std::thread::sleep(Duration::from_millis(50));
    }
    let seen=requests.lock().unwrap();
    assert_eq!(seen.len(),before+1);
    assert_eq!(seen.last().unwrap()["reasoning"]["effort"],"ultra");
}

#[test]
fn command_failure_persists_actual_evidence() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let saw_tool = request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["role"] == "tool");
        if saw_tool {
            vec![
                json!({"choices":[{"delta":{"content":"명령 실패를 확인했습니다."},"finish_reason":"stop"}]}),
            ]
        } else {
            vec![
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"failed-shell","type":"function","function":{"name":"shell_exec","arguments":"{\"command\":\"exit 7\"}"}}]},"finish_reason":"tool_calls"}]}),
            ]
        }
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let session = app.post("/api/sessions", &json!({"title":"failure"}));
    let stream =
        app.chat(&json!({"sessionId":session["id"],"input":"실패 명령 실행","autoLearning":false}));
    assert!(stream.contains("tool_error"), "{stream}");
    assert!(!stream.contains("\"type\":\"tool_end\""));
    let executions = app.get("/api/executions");
    let operations = app.get(&format!(
        "/api/executions/{}/operations",
        executions[0]["id"].as_str().unwrap()
    ));
    assert_eq!(operations[0]["status"], "failed");
    assert_eq!(operations[0]["arguments"]["command"], "exit 7");
    assert_eq!(operations[0]["result"]["exitCode"], 7);
}

#[test]
fn compatibility_tools_are_available_and_round_limit_is_a_failure() {
    let _serial = serial();
    let (port, requests) = scripted_provider(|_| {
        vec![
            json!({"choices":[{"delta":{"content":"<tool_call>{\"name\":\"workspace_list\",\"arguments\":{}}</tool_call>"},"finish_reason":"stop"}]}),
        ]
    });
    let app = TestApp::new(port, "compat");
    let session = app.post("/api/sessions", &json!({"title":"limit"}));
    let stream =
        app.chat(&json!({"sessionId":session["id"],"input":"계속 확인","autoLearning":false}));
    // Repeating a read call carries information, so the harness lets the
    // model continue until the round budget; it still must fail and never
    // save a raw tool-call fragment as the final answer.
    assert!(stream.contains("tool_start") || stream.contains("tool_error"), "{stream}");
    assert!(!stream.contains("assistant_saved"), "{stream}");
    let calls = requests.lock().unwrap();
    assert!(calls.len() >= 8, "round budget was not reached: {} calls", calls.len());
    assert!(calls[0]["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("workspace_list"));
    assert!(calls[0].get("tools").is_none());
    assert_eq!(app.get("/api/executions")[0]["status"], "failed");
    assert_eq!(app.get("/api/executions")[0]["taskOutcome"], "failed");
    let messages = app.get(&format!(
        "/api/sessions/{}/messages",
        session["id"].as_str().unwrap()
    ));
    assert!(messages
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["role"] == "assistant")
        .all(|row| row["tool_calls"].is_array()));
}

#[test]
fn cancelling_a_stalled_model_does_not_wait_for_network_data() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        std::thread::sleep(Duration::from_secs(5));
        vec![json!({"choices":[{"delta":{"content":"too late"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let session = app.post("/api/sessions", &json!({"title":"cancel"}));
    let id = session["id"].as_str().unwrap().to_string();
    let base = app.base.clone();
    let chat_id = id.clone();
    let started = Instant::now();
    let turn = std::thread::spawn(move || {
        reqwest::blocking::Client::new()
            .post(format!("{base}/api/chat"))
            .json(&json!({"sessionId":chat_id,"input":"wait","autoLearning":false}))
            .send()
            .unwrap()
            .text()
            .unwrap()
    });
    while app
        .get("/api/executions/active")
        .as_array()
        .unwrap()
        .is_empty()
    {
        assert!(started.elapsed() < Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(20));
    }
    app.post(&format!("/api/sessions/{id}/cancel"), &json!({}));
    let stream = turn.join().unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "cancellation stalled"
    );
    assert!(stream.contains("중단"));
    assert!(!stream.contains("too late"));
    assert_eq!(app.get("/api/executions")[0]["status"], "cancelled");
    assert_eq!(app.get("/api/executions")[0]["taskOutcome"], "cancelled");
}

#[test]
fn disabled_tools_cannot_be_enabled_by_the_model() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"forbidden","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"forbidden.txt\",\"content\":\"should never exist\"}"}}]},"finish_reason":"tool_calls"}]}),
        ]
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let stream = app.chat(&json!({"input":"대화만","toolsEnabled":false,"autoLearning":false}));
    assert!(stream.contains("도구가 비활성"), "{stream}");
    assert!(!app._root.join("forbidden.txt").exists());
    assert!(app.get("/api/approvals").as_array().unwrap().is_empty());
    assert_eq!(app.get("/api/executions")[0]["status"], "failed");
}

#[test]
fn a_room_greeting_reaches_the_whole_discussion_room() {
    let _serial = serial();
    let (port, requests) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"안녕하세요, 주인님."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    for (id, name) in [("a", "Alice"), ("b", "Bob")] {
        app.post(
            "/api/agents",
            &json!({"id":id,"name":name,"systemPrompt":"독립 참가자","enabled":true}),
        );
    }
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let room = app.post(
        "/api/rooms",
        &json!({"name":"인사방","agentIds":["a","b"],"routingMode":"discussion","rounds":1,"maxTurns":9,"toolsEnabled":true,"autoLearning":false}),
    );
    let id = room["id"].as_str().unwrap();
    let sent = app.post(
        &format!("/api/rooms/{id}/messages"),
        &json!({"input":"안녕","clientMessageId":"owner-greeting-001"}),
    );
    let job_id = sent["jobId"].as_str().unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let job = app.get(&format!("/api/jobs/{job_id}"));
        let status = job["status"].as_str().unwrap_or("");
        if matches!(status, "completed" | "failed") {
            assert_eq!(status, "completed", "{job}");
            break;
        }
        assert!(Instant::now() < deadline, "{job}");
        std::thread::sleep(Duration::from_millis(25));
    }
    let messages = app.get(&format!("/api/rooms/{id}/messages"));
    let assistants = messages
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["role"] == "assistant")
        .collect::<Vec<_>>();
    assert_eq!(assistants.len(), 2, "{messages}");
    assert_eq!(assistants[0]["agent_id"], "a", "{messages}");
    assert_eq!(assistants[1]["agent_id"], "b", "{messages}");
    let captured = requests.lock().unwrap();
    assert_eq!(
        captured.len(),
        2,
        "discussion mode must reach every configured participant"
    );
    let system = captured[0]["messages"][0]["content"]
        .as_str()
        .unwrap_or("");
    assert!(system.contains("자연스럽게 응답하십시오"), "{system}");
    assert!(system.contains("자신의 기여만 하십시오"), "{system}");
    assert!(
        !system.contains("If the owner only greets you"),
        "the harness must not dictate the reply wording: {system}"
    );
}

#[test]
fn room_debate_knowledge_is_recalled_by_a_different_model_with_provenance() {
    let _serial = serial();
    let (port, requests) = scripted_provider(|request| {
        let system = request["messages"][0]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        if request["stream"].as_bool() != Some(true) {
            if !system.contains("extract durable knowledge") {
                return vec![json!({"choices":[{"delta":{"content":"{}"},"finish_reason":"stop"}]})];
            }
            let prompt = request["messages"]
                .as_array()
                .and_then(|messages| messages.last())
                .and_then(|message| message["content"].as_str())
                .unwrap_or("");
            let parsed: Value = serde_json::from_str(prompt).unwrap_or_else(|_| json!({}));
            let messages = parsed["messages"].as_array().cloned().unwrap_or_default();
            let find = |needle: &str| {
                messages.iter().find(|message| {
                    message["content"]
                        .as_str()
                        .is_some_and(|content| content.contains(needle))
                })
            };
            let claim = find("배포는 X");
            let correction = find("배포는 Y");
            let (Some(claim), Some(correction)) = (claim, correction) else {
                return vec![json!({"choices":[{"delta":{"content":"{\"knowledge\":[]}"},"finish_reason":"stop"}]})];
            };
            let claim_id = claim["messageId"].as_str().unwrap_or("");
            let correction_id = correction["messageId"].as_str().unwrap_or("");
            let claim_text = claim["content"].as_str().unwrap_or("");
            let correction_text = correction["content"].as_str().unwrap_or("");
            let knowledge = json!({"knowledge":[
                {
                    "type":"conclusion","topicKey":"deploy-method",
                    "text":"배포는 X 방식","sourceMessageIds":[claim_id],
                    "evidenceQuotes":[claim_text],"confidence":0.5,
                    "verification":"debate"
                },
                {
                    "type":"correction","topicKey":"deploy-method",
                    "text":"배포는 Y 방식","sourceMessageIds":[correction_id],
                    "evidenceQuotes":[correction_text],"correctsMessageIds":[claim_id],
                    "confidence":0.9,"verification":"debate"
                }
            ]});
            return vec![json!({"choices":[{"delta":{"content":knowledge.to_string()},"finish_reason":"stop"}]})];
        }
        let model = request["model"].as_str().unwrap_or("");
        let reply = if model.contains("model-b") {
            "정정: X는 실패했다. 배포는 Y 방식으로 해야 한다."
        } else if model.contains("model-d") {
            "Y 방식입니다."
        } else {
            "배포는 X 방식으로 하자."
        };
        vec![json!({"choices":[{"delta":{"content":reply},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let provider = app.get("/api/providers")[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    app.post(
        "/api/providers",
        &json!({
            "name":"learner",
            "baseUrl":format!("http://127.0.0.1:{port}/v1"),
            "model":"mock",
            "reflection":true,
            "reflectionModel":"mock",
            "enabled":true,
            "toolMode":"native"
        }),
    );
    let agent_a = app.post(
        "/api/agents",
        &json!({"name":"A","systemPrompt":"A","providerId":provider,"model":"model-a"}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    let agent_b = app.post(
        "/api/agents",
        &json!({"name":"B","systemPrompt":"B","providerId":provider,"model":"model-b"}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    let room = app.post(
        "/api/rooms",
        &json!({
            "name":"knowledge-room","agentIds":[agent_a,agent_b],
            "routingMode":"discussion","rounds":1,"maxTurns":3,
            "toolsEnabled":false,"autoLearning":true
        }),
    );
    let room_id = room["id"].as_str().unwrap().to_string();
    let deadline = Instant::now() + Duration::from_secs(20);
    let first = app.post(
        &format!("/api/rooms/{room_id}/messages"),
        &json!({"input":"배포 방식을 정하자","clientMessageId":"owner-knowledge-001"}),
    );
    let job_id = first["jobId"].as_str().unwrap().to_string();
    loop {
        let job = app.get(&format!("/api/jobs/{job_id}"));
        if matches!(job["status"].as_str(), Some("completed" | "failed")) {
            assert_eq!(job["status"], "completed", "{job}");
            break;
        }
        assert!(Instant::now() < deadline, "{job}");
        std::thread::sleep(Duration::from_millis(50));
    }
    // The completed exchange is learned as one room-level unit.
    loop {
        let jobs = app.get("/api/jobs");
        let learn = jobs
            .as_array()
            .unwrap()
            .iter()
            .find(|job| {
                job["kind"] == "learn"
                    && job["input"]["roomLearning"] == true
                    && job["input"]["roomId"].as_str() == Some(room_id.as_str())
            });
        if let Some(job) = learn {
            if matches!(job["status"].as_str(), Some("completed" | "failed")) {
                assert_eq!(job["status"], "completed", "{job}");
                break;
            }
        }
        assert!(Instant::now() < deadline, "room learning did not finish: {jobs}");
        std::thread::sleep(Duration::from_millis(50));
    }
    let knowledge = app.get(&format!("/api/rooms/{room_id}/knowledge"));
    let items = knowledge.as_array().unwrap();
    let correction = items
        .iter()
        .find(|item| item["type"] == "correction")
        .expect("correction");
    assert_eq!(correction["status"], "active", "{correction}");
    assert!(correction["text"].as_str().unwrap_or("").contains("Y"));
    let sources = correction["sources"].as_array().unwrap();
    assert!(sources.iter().any(|source| source["model"] == "model-b"), "{sources:?}");
    let claim = items
        .iter()
        .find(|item| item["type"] == "claim")
        .expect("single-agent conclusion demoted to claim");
    assert_eq!(claim["status"], "superseded", "{claim}");

    // A new model joins the same room. It must see the correction and its
    // provenance even though it never spoke in the original debate.
    let agent_d = app.post(
        "/api/agents",
        &json!({"name":"D","systemPrompt":"D","providerId":provider,"model":"model-d"}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    app.post(
        &format!("/api/rooms/{room_id}"),
        &json!({
            "name":"knowledge-room",
            "agentIds":[agent_a,agent_b,agent_d],
            "routingMode":"discussion",
            "rounds":1,
            "maxTurns":6,
            "toolsEnabled":false,
            "autoLearning":true
        }),
    );
    let second = app.post(
        &format!("/api/rooms/{room_id}/messages"),
        &json!({"input":"우리가 정한 배포 방식은?","clientMessageId":"owner-knowledge-002"}),
    );
    let second_job = second["jobId"].as_str().unwrap().to_string();
    let second_deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let job = app.get(&format!("/api/jobs/{second_job}"));
        if matches!(job["status"].as_str(), Some("completed" | "failed")) {
            assert_eq!(job["status"], "completed", "{job}");
            break;
        }
        assert!(Instant::now() < second_deadline, "{job}");
        std::thread::sleep(Duration::from_millis(50));
    }
    let captured = requests.lock().unwrap();
    let d_prompt = captured
        .iter()
        .filter(|request| request["model"].as_str() == Some("model-d"))
        .filter_map(|request| {
            let messages = request["messages"].as_array()?;
            let has_owner = messages.iter().any(|message| {
                message["role"] == "user"
                    && message["content"]
                        .as_str()
                        .is_some_and(|content| content.contains("우리가 정한 배포 방식"))
            });
            // Retrieved knowledge is quoted data, not a system instruction.
            has_owner.then(|| {
                messages.iter().filter_map(|message|message["content"].as_str()).collect::<Vec<_>>().join("\n")
            })
        })
        .next()
        .expect("model D turn");
    assert!(d_prompt.contains("Shared room knowledge"), "{d_prompt}");
    assert!(d_prompt.contains("배포는 Y 방식"), "{d_prompt}");
    assert!(d_prompt.contains("model-b"), "{d_prompt}");
}

#[test]
fn room_runs_on_an_explicitly_selected_local_runtime() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let model = request["model"].as_str().unwrap_or("");
        assert_eq!(model, "local-room-model", "{request}");
        vec![json!({
            "choices":[{"delta":{"content":"LOCAL_ROOM_OK"},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":30,"completion_tokens":3,"total_tokens":33}
        })]
    });
    let app = TestApp::new(port, "compat");
    let local = app.post(
        "/api/providers",
        &json!({
            "name":"local room runtime",
            "kind":"ollama",
            "local":true,
            "baseUrl":format!("http://127.0.0.1:{port}/v1"),
            "model":"local-room-model",
            "enabled":true,
            "toolMode":"compat"
        }),
    );
    let agent = app.post(
        "/api/agents",
        &json!({
            "name":"local speaker",
            "systemPrompt":"LOCAL_SPEAKER_IDENTITY",
            "providerId":local["id"],
            "model":"local-room-model"
        }),
    );
    let room = app.post(
        "/api/rooms",
        &json!({
            "name":"local-room",
            "agentIds":[agent["id"]],
            "routingMode":"discussion",
            "rounds":1,
            "maxTurns":1,
            "toolsEnabled":false,
            "autoLearning":false,
            "autoModels":false,
            "models":[{"providerId":local["id"],"model":"local-room-model"}]
        }),
    );
    let room_id = room["id"].as_str().unwrap().to_string();
    let submitted = app.post(
        &format!("/api/rooms/{room_id}/messages"),
        &json!({"input":"로컬 모델로 답해","clientMessageId":"local-room-turn"}),
    );
    let job_id = submitted["jobId"].as_str().unwrap().to_string();
    let session_id = submitted["sessionId"].as_str().unwrap().to_string();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let job = app.get(&format!("/api/jobs/{job_id}"));
        if matches!(job["status"].as_str(), Some("completed" | "failed")) {
            assert_eq!(job["status"], "completed", "{job}");
            break;
        }
        assert!(Instant::now() < deadline, "{job}");
        std::thread::sleep(Duration::from_millis(50));
    }
    let messages = app.get(&format!("/api/sessions/{session_id}/messages"));
    let reply = messages
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|message| message["role"] == "assistant")
        .expect("local room reply");
    assert_eq!(reply["content"], "LOCAL_ROOM_OK", "{reply}");
    assert_eq!(reply["metadata"]["usage"]["providerId"], local["id"], "{reply}");
    assert_eq!(
        reply["metadata"]["usage"]["model"],
        "local-room-model",
        "{reply}"
    );
}

#[test]
fn social_room_exchanges_spend_no_learning_model_call() {
    let _serial = serial();
    let (port, requests) = scripted_provider(|request| {
        let system = request["messages"][0]["content"]
            .as_str()
            .unwrap_or("");
        if request["stream"].as_bool() != Some(true)
            && system.contains("extract durable knowledge")
        {
            return vec![json!({"choices":[{"delta":{"content":"{\"knowledge\":[]}"},"finish_reason":"stop"}]})];
        }
        vec![json!({"choices":[{"delta":{"content":"안녕하세요, 주인님."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let provider = app.get("/api/providers")[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    app.post(
        "/api/providers",
        &json!({
            "name":"learner",
            "baseUrl":format!("http://127.0.0.1:{port}/v1"),
            "model":"mock",
            "reflection":true,
            "reflectionModel":"mock",
            "enabled":true,
            "toolMode":"native"
        }),
    );
    let agent = app.post(
        "/api/agents",
        &json!({"name":"A","systemPrompt":"A","providerId":provider,"model":"mock"}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    let room = app.post(
        "/api/rooms",
        &json!({
            "name":"greeting-room","agentIds":[agent],
            "routingMode":"discussion","rounds":1,"maxTurns":1,
            "toolsEnabled":false,"autoLearning":true
        }),
    );
    let room_id = room["id"].as_str().unwrap().to_string();
    let submit_and_wait = |input: &str, client_id: &str| {
        let deadline = Instant::now() + Duration::from_secs(20);
        let submitted = app.post(
            &format!("/api/rooms/{room_id}/messages"),
            &json!({"input":input,"clientMessageId":client_id}),
        );
        let job_id = submitted["jobId"].as_str().unwrap().to_string();
        loop {
            let job = app.get(&format!("/api/jobs/{job_id}"));
            if matches!(job["status"].as_str(), Some("completed" | "failed")) {
                assert_eq!(job["status"], "completed", "{job}");
                break;
            }
            assert!(Instant::now() < deadline, "{job}");
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    submit_and_wait("다시 인사해봐", "greet-1");
    submit_and_wait("또 인사해봐", "greet-2");
    submit_and_wait("한 번 더 인사해봐", "greet-3");

    let jobs = app.get("/api/jobs");
    let learning_jobs = jobs
        .as_array()
        .unwrap()
        .iter()
        .filter(|job| {
            job["kind"] == "learn"
                && job["input"]["roomLearning"] == true
                && job["input"]["roomId"].as_str() == Some(room_id.as_str())
        })
        .count();
    assert_eq!(learning_jobs, 0, "greetings queued semantic learning: {jobs}");
    let extraction_calls = requests
        .lock()
        .unwrap()
        .iter()
        .filter(|request| {
            request["messages"][0]["content"]
                .as_str()
                .is_some_and(|system| system.contains("extract durable knowledge"))
        })
        .count();
    assert_eq!(extraction_calls, 0, "greetings called the learning model");

    // The gate must not block a real topic.
    submit_and_wait("배포 방식을 정하자", "topic-1");
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let jobs = app.get("/api/jobs");
        if let Some(job) = jobs.as_array().unwrap().iter().find(|job| {
            job["kind"] == "learn"
                && job["input"]["roomLearning"] == true
                && job["input"]["roomId"].as_str() == Some(room_id.as_str())
        }) {
            if matches!(job["status"].as_str(), Some("completed" | "failed")) {
                assert_eq!(job["status"], "completed", "{job}");
                break;
            }
        }
        assert!(Instant::now() < deadline, "topic turn did not learn: {jobs}");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        requests
            .lock()
            .unwrap()
            .iter()
            .any(|request| {
                request["messages"][0]["content"]
                    .as_str()
                    .is_some_and(|system| system.contains("extract durable knowledge"))
            }),
        "a topic turn must still reach the extractor"
    );
}

#[test]
fn invalid_automations_and_workflows_are_rejected_before_execution() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"ok"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let provider = app.get("/api/providers")[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let agent = app.post(
        "/api/agents",
        &json!({"name":"A","systemPrompt":"A","providerId":provider,"model":"mock"}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    let status = |path: &str, body: &Value| -> u16 {
        app.client
            .post(format!("{}{path}", app.base))
            .json(body)
            .send()
            .unwrap()
            .status()
            .as_u16()
    };
    assert_eq!(
        status(
            "/api/automations",
            &json!({"kind":"prompt","input":{"prompt":"x","agentId":""},"intervalMinutes":10,"enabled":true})
        ),
        400,
        "an automation without an existing agent must not be saved"
    );
    let automation = app.post(
        "/api/automations",
        &json!({"kind":"prompt","input":{"prompt":"x","agentId":agent},"intervalMinutes":10,"enabled":true}),
    );
    assert!(automation["id"].is_string(), "{automation}");
    assert_eq!(
        status(
            "/api/workflows",
            &json!({"name":"W","steps":[{"name":"s","prompt":"","agentId":agent}]})
        ),
        400,
        "a workflow with an empty step prompt must not be saved"
    );
    let workflow = app.post(
        "/api/workflows",
        &json!({"name":"W","steps":[{"name":"s","prompt":"do","agentId":agent}]}),
    );
    assert!(workflow["id"].is_string(), "{workflow}");
    let skill = app.post(
        "/api/skills",
        &json!({"id":"skill-contract","name":"S","instructions":"i","enabled":true,"status":"draft"}),
    );
    assert_eq!(skill["status"], "active", "{skill}");
}

#[test]
fn persistent_room_http_flow_keeps_owner_once_and_shares_peer_context() {
    let _serial = serial();
    let (port, requests) = scripted_provider(|request| {
        let system = request["messages"][0]["content"].as_str().unwrap();
        let reply = if system.contains("활성 에이전트: Alice (id: a)") {
            "Alice의 실제 의견"
        } else {
            "Bob이 Alice의 의견을 확인했습니다"
        };
        vec![json!({"choices":[{"delta":{"content":reply},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    for (id, name) in [("a", "Alice"), ("b", "Bob")] {
        app.post(
            "/api/agents",
            &json!({"id":id,"name":name,"systemPrompt":"독립 참가자","enabled":true}),
        );
    }
    let room=app.post("/api/rooms",&json!({"name":"공용 대화","agentIds":["a","b"],"routingMode":"discussion","rounds":1,"maxTurns":2,"toolsEnabled":false,"autoLearning":false}));
    let id = room["id"].as_str().unwrap();
    let input = json!({"input":"우리 함께 의견을 나눠보자","clientMessageId":"owner-request-001"});
    let sent = app.post(&format!("/api/rooms/{id}/messages"), &input);
    let retry = app.post(&format!("/api/rooms/{id}/messages"), &input);
    assert_eq!(sent["jobId"], retry["jobId"]);
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let state = app.get(&format!("/api/rooms/{id}/state"));
        let status = state["jobs"][0]["status"].as_str().unwrap();
        if status == "completed" {
            break;
        }
        assert!(matches!(status, "queued" | "running"), "{state}");
        assert!(Instant::now() < deadline, "{state}");
        std::thread::sleep(Duration::from_millis(25));
    }
    let messages = app.get(&format!("/api/rooms/{id}/messages"));
    let rows = messages.as_array().unwrap();
    assert_eq!(rows.iter().filter(|row| row["role"] == "user").count(), 1);
    assert_eq!(rows[0]["metadata"]["authorKind"], "owner");
    let agents = rows
        .iter()
        .filter(|row| row["role"] == "assistant")
        .map(|row| row["agent_id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(agents, vec!["a", "b"]);
    let captured = requests.lock().unwrap();
    assert_eq!(captured.len(), 2);
    assert!(captured[1]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|message| message["content"]
            .as_str()
            .unwrap_or("")
            .contains("Alice의 실제 의견")));
    drop(captured);
    // One owner message is one root task; each speaker is an attempt, not a
    // new independent task.
    let session_id = room["sessionId"].as_str().unwrap();
    let executions = app.get("/api/executions");
    let room_runs = executions
        .as_array()
        .unwrap()
        .iter()
        .filter(|execution| execution["sessionId"] == session_id)
        .collect::<Vec<_>>();
    assert_eq!(room_runs.len(), 2, "{executions}");
    let task_id = room_runs[0]["taskId"].as_str().unwrap();
    assert!(room_runs
        .iter()
        .all(|execution| execution["taskId"] == task_id));
    let task = app.get(&format!("/api/tasks/{task_id}"));
    assert_eq!(task["id"], task_id);
    assert_eq!(task["contract"]["family"], "room");
    assert_eq!(task["contract"]["source"], "owner");
    assert_eq!(task["sourceMessageId"], rows[0]["id"]);
    assert_eq!(task["attempts"].as_array().unwrap().len(), 2);
    let speakers = task["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|attempt| attempt["agentId"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(speakers, vec!["a", "b"]);
    assert_eq!(task["attempts"][0]["verificationOwner"], "self");
}

#[test]
fn room_selected_tools_keep_read_write_history_and_require_real_write_approval() {
    let _serial = serial();
    let (port, requests) = scripted_provider(|request| {
        let observations = request["messages"].as_array().unwrap().iter()
            .filter(|message|message["role"]=="tool").count();
        let call = match observations {
            0 => Some(("read","workspace_read",json!({"path":"input.txt"}))),
            1 => Some(("write","workspace_write",json!({"path":"output.txt","content":"copied evidence"}))),
            _ => None,
        };
        match call {
            Some((id,name,arguments)) => vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":id,"type":"function","function":{"name":name,"arguments":arguments.to_string()}}]},"finish_reason":"tool_calls"}]})],
            None => vec![json!({"choices":[{"delta":{"content":"확인한 근거를 기록했습니다."},"finish_reason":"stop"}]})],
        }
    });
    let app = TestApp::new(port,"native");
    std::fs::write(app._root.join("input.txt"),"copied evidence").unwrap();
    app.post("/api/agents",&json!({"id":"a","name":"Reader","systemPrompt":"Inspect the requested file and record the evidence."}));
    let room=app.post("/api/rooms",&json!({"name":"selected tools","agentIds":["a"],"rounds":1,"maxTurns":1,"toolsEnabled":true,"autoLearning":false}));
    let sent=app.post(&format!("/api/rooms/{}/messages",room["id"].as_str().unwrap()),&json!({"input":"테스트 파일을 확인하고 결과를 기록해줘"}));
    let approval=app.wait_pending_approval(15);
    assert_eq!(approval["tool"],"workspace_write");
    assert!(!app._root.join("output.txt").exists());
    app.post("/api/approvals",&json!({"id":approval["id"],"allow":true}));
    let deadline=Instant::now()+Duration::from_secs(20);
    loop {
        let job=app.get(&format!("/api/jobs/{}",sent["jobId"].as_str().unwrap()));
        if job["status"]=="completed" { break; }
        assert!(matches!(job["status"].as_str(),Some("queued"|"running")),"{job}");
        assert!(Instant::now()<deadline,"{job}");
        std::thread::sleep(Duration::from_millis(30));
    }
    assert_eq!(std::fs::read_to_string(app._root.join("output.txt")).unwrap(),"copied evidence");
    let requests=requests.lock().unwrap();
    assert_eq!(requests.len(),3);
    let tools = requests[0]["tools"].as_array().unwrap();
    let tool_names = tools.iter().filter_map(|tool| tool["function"]["name"].as_str()).collect::<Vec<_>>();
    assert!(tool_names.contains(&"workspace_read"), "{tool_names:?}");
    assert!(tool_names.contains(&"workspace_write"), "{tool_names:?}");
    assert!(!tool_names.contains(&"dominion_tool"), "{tool_names:?}");
    assert!(tool_names.len() < 19, "casual/work tool selection must not ship the full registry: {tool_names:?}");
    assert!(requests[1]["messages"].as_array().unwrap().iter().any(|message|message["tool_calls"][0]["function"]["name"]=="workspace_read"));
    // Runtime hints must never sit between an assistant tool call and its
    // result; Beam rejects that ordering as an unanswered call.
    for request in requests.iter() {
        let messages = request["messages"].as_array().unwrap();
        for (index, message) in messages.iter().enumerate() {
            if message["role"] == "assistant"
                && message["tool_calls"].as_array().is_some_and(|calls| !calls.is_empty())
            {
                assert_eq!(
                    messages.get(index + 1).map(|next| next["role"].as_str().unwrap_or("")),
                    Some("tool"),
                    "tool call must be followed by its result: {messages:?}"
                );
            }
        }
    }
    let execution=app.get("/api/executions")[0].clone();
    assert_eq!(execution["inputContext"]["contextModelCalls"],0);
    assert!(execution["inputContext"]["personaTokens"].as_u64().unwrap()>0);
    assert_eq!(execution["toolCalls"],2);
}

#[test]
fn room_selects_and_executes_an_mcp_tool_through_approval() {
    let _serial = serial();
    let (port, requests) = scripted_provider(|request| {
        let observations = request["messages"].as_array().unwrap().iter()
            .filter(|message|message["role"]=="tool").count();
        if observations == 0 {
            vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"mcp-call","type":"function","function":{"name":"mcp__fake_mcp__search_issues","arguments":json!({"query":"repo:dominion"}).to_string()}}]},"finish_reason":"tool_calls"}]})]
        } else {
            vec![json!({"choices":[{"delta":{"content":"MCP 검색 결과를 확인했습니다."},"finish_reason":"stop"}]})]
        }
    });
    let app = TestApp::new(port, "native");
    let script = app._root.join("fake_mcp.py");
    std::fs::write(
        &script,
        r#"import json, sys
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        message = json.loads(line)
    except Exception:
        continue
    if "id" not in message:
        continue
    method = message.get("method")
    if method == "initialize":
        result = {"protocolVersion": "2024-11-05", "capabilities": {"tools": {}}, "serverInfo": {"name": "fake", "version": "1.0"}}
    elif method == "tools/list":
        result = {"tools": [{"name": "search_issues", "description": "Search GitHub issues and pull requests", "inputSchema": {"type": "object", "properties": {"query": {"type": "string"}}, "required": ["query"]}}]}
    elif method == "tools/call":
        result = {"content": [{"type": "text", "text": "2 issues matched the query"}]}
    else:
        result = {}
    print(json.dumps({"jsonrpc": "2.0", "id": message["id"], "result": result}), flush=True)
"#,
    )
    .unwrap();
    let conn = Connection::open(app._root.join("runtime/dominion.sqlite")).unwrap();
    conn.busy_timeout(Duration::from_secs(5)).unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO records(kind,id,data) VALUES('mcp','fake',?1)",
        [json!({
            "id":"fake","name":"Fake MCP","enabled":true,"transport":"stdio",
            "command":"python3","args":[script.to_string_lossy()]
        }).to_string()],
    )
    .unwrap();
    drop(conn);

    let discovered = app.get("/api/mcp/discover");
    assert!(discovered.as_array().unwrap().iter().any(|tool| tool["name"] == "mcp__fake_mcp__search_issues"), "{discovered}");

    app.post("/api/agents",&json!({"id":"a","name":"Researcher","systemPrompt":"Use available tools and report results accurately."}));
    let room=app.post("/api/rooms",&json!({"name":"mcp tools","agentIds":["a"],"rounds":1,"maxTurns":1,"toolsEnabled":true,"autoLearning":false}));
    let sent=app.post(&format!("/api/rooms/{}/messages",room["id"].as_str().unwrap()),&json!({"input":"github 이슈를 검색해"}));
    let approval=app.wait_pending_approval(20);
    assert_eq!(approval["tool"],"mcp__fake_mcp__search_issues");
    app.post("/api/approvals",&json!({"id":approval["id"],"allow":true}));
    let deadline=Instant::now()+Duration::from_secs(25);
    loop {
        let job=app.get(&format!("/api/jobs/{}",sent["jobId"].as_str().unwrap()));
        if job["status"]=="completed" { break; }
        assert!(matches!(job["status"].as_str(),Some("queued"|"running")),"{job}");
        assert!(Instant::now()<deadline,"{job}");
        std::thread::sleep(Duration::from_millis(30));
    }
    let requests=requests.lock().unwrap();
    assert!(requests.len()>=2,"{requests:?}");
    let tools=requests[0]["tools"].as_array().unwrap();
    let names=tools.iter().filter_map(|tool|tool["function"]["name"].as_str()).collect::<Vec<_>>();
    assert!(names.contains(&"mcp__fake_mcp__search_issues"),"{names:?}");
    assert!(!names.contains(&"dominion_tool"),"{names:?}");
    assert!(!names.contains(&"workspace_write"),"unrelated tools must not ship with an MCP request: {names:?}");
    assert!(requests[1]["messages"].as_array().unwrap().iter().any(|message|message["tool_calls"][0]["function"]["name"]=="mcp__fake_mcp__search_issues"));
    assert!(requests[1]["messages"].as_array().unwrap().iter().any(|message|message["role"]=="tool" && message["content"].as_str().is_some_and(|text|text.contains("2 issues matched"))));
    let execution=app.get("/api/executions")[0].clone();
    assert_eq!(execution["toolCalls"],1);
    assert_eq!(execution["status"],"completed");
}

#[test]
fn room_system_prompt_preview_returns_the_korean_contract_and_persona() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"ok"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    app.post("/api/agents", &json!({"id":"a","name":"미리보기","systemPrompt":"테스트 페르소나 원문입니다."}));
    app.post("/api/rooms", &json!({
        "id":"prompt-room","name":"prompt room","agentIds":["a"],
        "rounds":1,"maxTurns":1,"toolsEnabled":false,"autoLearning":false
    }));
    let preview = app.get("/api/rooms/prompt-room/system-prompt?agentId=a");
    let prompt = preview["prompt"].as_str().unwrap();
    assert!(prompt.contains("주인이 결정권을 가집니다."), "{prompt}");
    assert!(prompt.contains("동료 메시지, 스크린샷, 인용 예시, 도구 출력"), "{prompt}");
    assert!(prompt.contains("테스트 페르소나 원문입니다."), "{prompt}");
    assert!(preview["chars"].as_u64().unwrap() > preview["personaChars"].as_u64().unwrap());
    assert!(preview["tokens"].as_u64().unwrap() > 0);
    let common = preview["common"].as_str().unwrap();
    assert!(common.contains("주인이 결정권을 가집니다."), "{common}");
    assert!(!common.contains("테스트 페르소나 원문입니다."), "{common}");
    assert_eq!(preview["persona"], "테스트 페르소나 원문입니다.");
    assert!(preview["commonChars"].as_u64().unwrap() > 0);
    assert!(preview["commonTokens"].as_u64().unwrap() > 0);
}

#[test]
fn a_short_greeting_reaches_all_nine_when_the_turn_cap_covers_the_roster() {
    let _serial = serial();
    let (port, requests) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"안녕하세요, 주인님."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let ids = (1..=9).map(|index| format!("a{index}")).collect::<Vec<_>>();
    for id in &ids {
        app.post("/api/agents", &json!({"id":id,"name":id,"systemPrompt":"인사만 하세요."}));
    }
    let room = app.post("/api/rooms", &json!({
        "id":"nine-greeting","name":"아홉 명","agentIds":ids,
        "routingMode":"discussion","rounds":1,"maxTurns":10,
        "toolsEnabled":false,"autoLearning":false
    }));
    let sent = app.post(&format!("/api/rooms/{}/messages",room["id"].as_str().unwrap()), &json!({"input":"다시 인사해봐"}));
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let job = app.get(&format!("/api/jobs/{}", sent["jobId"].as_str().unwrap()));
        if job["status"] == "completed" { break; }
        assert!(Instant::now() < deadline, "{job}");
        std::thread::sleep(Duration::from_millis(30));
    }
    let messages = app.get("/api/rooms/nine-greeting/messages");
    let rows = messages.as_array().unwrap();
    let speakers = rows
        .iter()
        .filter(|row| row["role"] == "assistant")
        .map(|row| row["agent_id"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(speakers.len(), 9, "only some participants replied: {speakers:?}");
    assert_eq!(
        rows.iter().filter(|row| row["role"] == "user").count(),
        1,
        "the harness must not fabricate owner messages"
    );
    assert_eq!(requests.lock().unwrap().len(), 9);
}

