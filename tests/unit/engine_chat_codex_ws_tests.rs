//! Extracted isolated regression tests for src/engine/chat/codex_ws.rs.
use super::*;

    fn item(role: &str, text: &str) -> Value {
        json!({"role":role,"content":[{"type":"text","text":text}]})
    }

    #[test]
    fn tool_item_signatures_include_call_identity_and_output() {
        let call_a = json!({
            "type":"function_call","call_id":"call-a",
            "name":"workspace_write","arguments":"{\"path\":\"a.txt\"}"
        });
        let call_b = json!({
            "type":"function_call","call_id":"call-b",
            "name":"workspace_write","arguments":"{\"path\":\"a.txt\"}"
        });
        let output = json!({
            "type":"function_call_output","call_id":"call-a","output":"ok"
        });
        assert_ne!(item_signature(&call_a), item_signature(&call_b));
        assert_ne!(item_signature(&call_a), item_signature(&output));
        assert_eq!(item_signature(&call_a).0, "function_call");
        assert_eq!(item_signature(&output).0, "function_call_output");
    }

    #[test]
    fn replacing_an_image_invalidates_the_remote_chain() {
        let first = json!({"role":"user","content":[
            {"type":"input_text","text":"describe"},
            {"type":"input_image","image_url":"data:image/png;base64,original"}
        ]});
        let mut edited = first.clone();
        edited["content"][1]["image_url"] = json!("data:image/png;base64,changed");
        let chain = vec![item_signature(&first)];
        let incoming = vec![
            item_signature(&edited),
            item_signature(&item("user", "next")),
        ];
        assert_eq!(plan_chain(&chain, &incoming), Plan::Full);
    }

    #[test]
    fn a_matching_prefix_sends_only_the_new_suffix() {
        let chain = vec![("user".to_string(), "one".to_string())];
        let incoming = vec![
            ("user".to_string(), "one".to_string()),
            ("assistant".to_string(), "two".to_string()),
        ];
        assert_eq!(plan_chain(&chain, &incoming), Plan::Delta(1));
    }

    #[test]
    fn edited_or_truncated_history_resends_everything() {
        let chain = vec![("user".to_string(), "one".to_string())];
        assert_eq!(
            plan_chain(&chain, &[("user".to_string(), "changed".to_string())]),
            Plan::Full
        );
        assert_eq!(plan_chain(&chain, &[]), Plan::Full);
        assert_eq!(
            plan_chain(&chain, &[("user".to_string(), "one".to_string())]),
            Plan::Full,
            "no new item means no incremental request"
        );
    }

    #[test]
    fn assistant_output_is_rebuilt_from_the_messages_the_next_call_will_send() {
        let response = json!({"output":[
            {"type":"message","role":"assistant","content":[{"type":"output_text","text":"안녕"}]},
            {"type":"reasoning","summary":[]}
        ]});
        let items = assistant_items(&response, "ignored");
        assert_eq!(items.len(), 1);
        assert_eq!(item_signature(&items[0]), ("assistant".to_string(), "안녕".to_string()));
    }

    #[test]
    fn websocket_url_keeps_the_codex_backend_path() {
        assert_eq!(
            websocket_url("https://chatgpt.com/backend-api/codex"),
            "wss://chatgpt.com/backend-api/codex/responses"
        );
        assert_eq!(websocket_url("http://127.0.0.1:1/v1/"), "ws://127.0.0.1:1/v1/responses");
    }

    #[test]
    fn cached_usage_is_unknown_when_absent_not_zero() {
        let completion = Completion::default();
        assert_eq!(completion.cached_prompt_tokens, None);
    }

    fn test_state() -> (AppState, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("dominion-ws-audit-{}", uuid::Uuid::new_v4()));
        let config = crate::config::Config {
            port: 0,
            data_dir: root.clone(),
            web_dir: root.join("web"),
            workspace: root.clone(),
        };
        let store = crate::db::Store::open(&config.db_path()).unwrap();
        let vault = crate::vault::Vault::open(&config.key_path());
        (AppState::new(config, store, vault), root)
    }

    async fn fixture(
        events: Vec<Value>,
    ) -> (
        ProviderRef,
        tokio::sync::mpsc::UnboundedReceiver<Value>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let provider = ProviderRef {
            id: uuid::Uuid::new_v4().to_string(),
            name: "fixture".into(),
            base_url: format!("http://{}", listener.local_addr().unwrap()),
            api_key: "fixture".into(),
            model: "fixture".into(),
            tool_mode: "native".into(),
            reasoning_protocol: "openai".into(),
            local: false,
            context_tokens: None,
            max_output_tokens: None,
            requests_per_minute: None,
            kind: "codex".into(),
            account_id: Some("fixture".into()),
        };
        let server = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                while let Some(Ok(Message::Text(text))) = socket.next().await {
                    tx.send(serde_json::from_str(&text).unwrap()).unwrap();
                    if events.is_empty() {
                        std::future::pending::<()>().await;
                    }
                    for event in &events {
                        socket.send(Message::Text(event.to_string())).await.unwrap();
                    }
                    if !events
                        .iter()
                        .any(|event| event["type"] == "response.completed")
                    {
                        let _ = socket.close(None).await;
                        break;
                    }
                }
            }
        });
        (provider, rx, server)
    }

    #[tokio::test]
    async fn unsupported_cache_hint_is_not_retried_as_a_broken_connection() {
        let (state,root)=test_state();
        let (provider,mut requests,server)=fixture(vec![json!({"type":"error","error":{
            "code":"invalid_parameter","param":"prompt_cache_key","message":"prompt_cache_key is not supported"}})]).await;
        let (tx,_)=tokio::sync::mpsc::unbounded_channel();
        let result=stream(&state,&provider,"fixture","fixture","session",
            &json!({"prompt_cache_key":"stable-key"}),vec![item("user","hello")],"session",&tx).await;
        assert!(matches!(result,Err(WsError::HintRejected(_))));
        assert_eq!(requests.recv().await.unwrap()["prompt_cache_key"],"stable-key");
        assert!(requests.try_recv().is_err(),"do not open another socket to send the same rejected hint");
        server.abort();std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn recycling_an_expired_socket_resends_the_full_history() {
        let (state, root) = test_state();
        let (provider, mut requests, server) = fixture(vec![
            json!({"type":"response.output_text.delta","delta":"reply"}),
            json!({"type":"response.completed","response":{"id":"resp-1","output":[]}}),
        ])
        .await;
        let (tx, _) = tokio::sync::mpsc::unbounded_channel();
        let first = vec![item("user", "original request")];
        stream(
            &state,
            &provider,
            "fixture",
            "fixture",
            "session",
            &json!({}),
            first.clone(),
            "session",
            &tx,
        )
        .await
        .unwrap();
        requests.recv().await.unwrap();
        let key = registry_key(&provider, "fixture", "session");
        chain_for(&key).lock().await.opened =
            Some(Instant::now() - MAX_CONNECTION_AGE - Duration::from_secs(1));
        let incoming = vec![
            first[0].clone(),
            item("assistant", "reply"),
            item("user", "follow up"),
        ];
        let outcome = stream(
            &state,
            &provider,
            "fixture",
            "fixture",
            "session",
            &json!({}),
            incoming.clone(),
            "session",
            &tx,
        )
        .await
        .unwrap();
        let second = requests.recv().await.unwrap();
        assert_eq!(
            second["input"],
            json!(incoming),
            "a new socket cannot inherit old items"
        );
        assert!(second.get("previous_response_id").is_none());
        assert_eq!(outcome.transport["mode"], "full");
        forget(&key);
        server.abort();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn interrupted_output_is_not_retried_or_accepted_as_complete() {
        for terminal in [
            None,
            Some(
                json!({"type":"response.incomplete","response":{"incomplete_details":{"reason":"max_output_tokens"}}}),
            ),
        ] {
            let (state, root) = test_state();
            let mut events = vec![json!({"type":"response.output_text.delta","delta":"partial"})];
            events.extend(terminal);
            let (provider, mut requests, server) = fixture(events).await;
            let (tx, mut output) = tokio::sync::mpsc::unbounded_channel();
            let result = stream(
                &state,
                &provider,
                "fixture",
                "fixture",
                "session",
                &json!({}),
                vec![item("user", "request")],
                "session",
                &tx,
            )
            .await;
            assert!(
                matches!(result, Err(WsError::Fatal(_))),
                "partial output must surface as failure"
            );
            requests.recv().await.unwrap();
            assert!(
                requests.try_recv().is_err(),
                "a partial generation must not be replayed"
            );
            assert_eq!(output.try_recv().unwrap()["text"], "partial");
            assert!(output.try_recv().is_err());
            forget(&registry_key(&provider, "fixture", "session"));
            server.abort();
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[tokio::test]
    async fn cancelling_a_stalled_socket_does_not_wait_for_network_data() {
        let (state, root) = test_state();
        let (provider, mut requests, server) = fixture(Vec::new()).await;
        let (tx, _) = tokio::sync::mpsc::unbounded_channel();
        let body = json!({});
        let request = stream(
            &state,
            &provider,
            "fixture",
            "fixture",
            "session",
            &body,
            vec![item("user", "request")],
            "session",
            &tx,
        );
        let cancel = async {
            requests.recv().await.unwrap();
            state.cancel_session("session");
        };
        let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(request, cancel)
        })
        .await
        .expect("cancellation must interrupt a stalled socket");
        assert!(matches!(result, Err(WsError::Fatal(_))));
        forget(&registry_key(&provider, "fixture", "session"));
        server.abort();
        std::fs::remove_dir_all(root).unwrap();
    }
