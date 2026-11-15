//! Extracted regression tests for src/engine/discord.rs.
use super::*;
    use crate::{config::Config, db::Store, vault::Vault};
    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-discord-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let state = AppState::new(
            cfg.clone(),
            Store::open(&cfg.db_path()).unwrap(),
            Vault::open(&cfg.key_path()),
        );
        state.store.put("agent", &json!({"id":"a","name":"가람"}));
        (state, dir)
    }
    fn config() -> Value {
        json!({"id":"c","kind":"discord","guildId":"10","channelId":"20","agentId":"a","mentionOnly":true})
    }
    fn message() -> Value {
        json!({"id":"100","channel_id":"20","author":{"id":"30","bot":false},"content":"안녕","mentions":[{"id":"40"}],"type":0})
    }
    #[test]
    fn routing_rejects_self_webhooks_other_channels_and_unapproved_bots() {
        let c = config();
        let mut m = message();
        assert!(accepts(&c, &m, "40"));
        m["author"]["id"] = json!("40");
        assert!(!accepts(&c, &m, "40"));
        m = message();
        m["channel_id"] = json!("21");
        assert!(!accepts(&c, &m, "40"));
        m = message();
        m["webhook_id"] = json!("33");
        assert!(!accepts(&c, &m, "40"));
        m = message();
        m["author"]["bot"] = json!(true);
        assert!(!accepts(&c, &m, "40"));
        let mut c = c;
        c["allowedBotIds"] = json!("30");
        assert!(accepts(&c, &m, "40"));
        m["mentions"] = json!([]);
        assert!(!accepts(&c, &m, "40"));
    }
    #[tokio::test]
    async fn room_binding_routes_once_through_canonical_ledger_without_private_agent_sessions() {
        let (state, dir) = state();
        state
            .store
            .put("room", &json!({"id":"room","name":"공용","agentIds":["a"]}));
        let mut config = config();
        config["roomId"] = json!("room");
        config["driver"] = json!("dominion-discord");
        config["revision"] = json!("r");
        config["enabled"] = json!(true);
        state.store.put("channel", &config);
        let job = json!({"id":"fixture-parent","input":{"connectionId":"c","configVersion":"r","botId":"40","message":message()}});
        let context = JobContext::new(state.clone(), "fixture-parent".into());
        let first = run(&context, &job).await.unwrap();
        let again = run(&context, &job).await.unwrap();
        assert_eq!(first["roomJobId"], again["roomJobId"]);
        assert_eq!(state.store.count("session"), 1);
        let sessions = state.store.all("session");
        assert_eq!(sessions[0]["roomId"], "room");
        let messages = state.store.messages(text(&sessions[0], "id"));
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["content"], "안녕");
        assert_eq!(messages[0]["metadata"]["origin"]["kind"], "discord");
        assert_eq!(state.store.count("job"), 1);
        let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn human_allowlist_and_mention_policy_are_enforced() {
        let mut c = config();
        let mut m = message();
        c["allowedUserIds"] = json!("31");
        assert!(!accepts(&c, &m, "40"));
        c["allowedUserIds"] = json!("30");
        assert!(accepts(&c, &m, "40"));
        m["mentions"] = json!([]);
        assert!(!accepts(&c, &m, "40"));
        c["mentionOnly"] = json!(false);
        assert!(accepts(&c, &m, "40"));
    }
    #[test]
    fn tools_require_explicit_trusted_users_and_rooms_are_bounded() {
        let (s, dir) = state();
        let mut c = config();
        assert!(validate(&s, &c).is_ok());
        c["toolsEnabled"] = json!(true);
        assert!(validate(&s, &c).is_err());
        c["allowedUserIds"] = json!("30");
        assert!(validate(&s, &c).is_ok());
        let mut agents=(0..MAX_PARTICIPANTS)
            .map(|index|format!("agent-{index}")).collect::<Vec<_>>();
        for id in &agents {
            s.store.put("agent",&json!({"id":id,"name":id}));
        }
        s.store.put("room",&json!({"id":"r","agentIds":agents}));
        c["roomId"] = json!("r");
        assert!(validate(&s,&c).is_ok(),
            "Discord must accept the room engine's 32-agent limit");
        agents.push("one-more".into());
        s.store.put("room",&json!({"id":"r","agentIds":agents}));
        assert!(validate(&s,&c).unwrap_err().contains("1~32"),
            "the 33rd participant still exceeds the shared limit");
        drop(s);
        let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn unicode_splitting_respects_discord_limit_without_loss() {
        let text = "안녕😀".repeat(1500);
        let parts = chunks(&text, 2000);
        assert!(parts.len() > 2);
        assert_eq!(parts.concat(), text);
        assert!(parts.iter().all(|p| p.encode_utf16().count() <= 2000));
    }
    #[test]
    fn named_room_routing_and_legacy_secret_migration() {
        let (s, dir) = state();
        s.store.put("agent", &json!({"id":"b","name":"나래"}));
        s.store.put("room", &json!({"id":"r","agentIds":["a","b"]}));
        let mut c = config();
        c["roomId"] = json!("r");
        let mut m = message();
        m["content"] = json!("@나래 의견은?");
        assert_eq!(participants(&s, &c, &m).unwrap(), vec!["b"]);
        c["token"] = json!("private-test-token");
        s.store.put("channel", &c);
        migrate_secrets(&s);
        assert!(s.store.get("channel", "c").unwrap().get("token").is_none());
        assert_eq!(
            s.read_secret(&token_name("c")).as_deref(),
            Some("private-test-token")
        );
        drop(s);
        let _ = std::fs::remove_dir_all(dir);
    }
    #[tokio::test]
    async fn rest_client_uses_bot_auth_and_safe_mentions_with_mock_transport() {
        use axum::{
            extract::State,
            http::HeaderMap,
            routing::{get, post},
            Json, Router,
        };
        use std::sync::{Arc, Mutex};
        let captured = Arc::new(Mutex::new(Vec::<Value>::new()));
        async fn me(headers: HeaderMap) -> Json<Value> {
            assert_eq!(headers.get("authorization").unwrap(), "Bot mock-token");
            Json(json!({"id":"40","bot":true}))
        }
        async fn send(
            State(out): State<Arc<Mutex<Vec<Value>>>>,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            out.lock().unwrap().push(body);
            Json(json!({"id":"200"}))
        }
        let app = Router::new()
            .route("/users/@me", get(me))
            .route(
                "/channels/20",
                get(|| async { Json(json!({"id":"20","guild_id":"10","type":0})) }),
            )
            .route("/channels/20/messages", post(send))
            .with_state(captured.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = DiscordClient {
            client: reqwest::Client::new(),
            token: "mock-token".into(),
            base: format!("http://{addr}"),
        };
        let (s, dir) = state();
        let mut c = config();
        c["enabled"] = json!(true);
        c["revision"] = json!("v1");
        s.store.put("channel", &c);
        assert_eq!(client.verify(&c).await.unwrap(), "40");
        let mut delivery = json!({"id":"d","status":"ready","content":"@everyone 답변😀","sent":0});
        deliver(
            &s,
            &client,
            &c,
            &message(),
            &json!({"name":"가람"}),
            &mut delivery,
        )
        .await
        .unwrap();
        let out = captured.lock().unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["allowed_mentions"]["parse"], json!([]));
        assert_eq!(out[0]["allowed_mentions"]["replied_user"], false);
        assert_eq!(out[0]["enforce_nonce"], true);
        assert!(text(&out[0], "nonce").len() <= 25);
        assert_eq!(delivery["status"], "delivered");
        drop(out);
        server.abort();
        drop(s);
        let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn approved_bot_dialogue_has_a_durable_minute_budget() {
        let (s, dir) = state();
        let c = config();
        let mut m = message();
        m["author"]["bot"] = json!(true);
        assert!(bot_budget(&s, &c, &m));
        for i in 0..2 {
            s.store.put("job",&json!({"id":format!("j{i}"),"kind":"discord","status":"queued","input":{"connectionId":"c","message":m}}));
        }
        assert!(!bot_budget(&s, &c, &m));
        m["author"]["bot"] = json!(false);
        assert!(bot_budget(&s, &c, &m));
        drop(s);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn rest_backfill_preserves_busy_channels_oldest_first() {
        use axum::{extract::Query, routing::get, Json, Router};
        async fn page(Query(q): Query<HashMap<String, String>>) -> Json<Value> {
            let (start, end) = if q.contains_key("after") {
                (201, 300)
            } else if q.get("before").map(String::as_str) == Some("201") {
                (101, 200)
            } else {
                (1, 100)
            };
            Json(json!((start..=end)
                .rev()
                .map(|i| json!({"id":i.to_string(),"channel_id":"20"}))
                .collect::<Vec<_>>()))
        }
        let app = Router::new().route("/channels/20/messages", get(page));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = DiscordClient {
            client: reqwest::Client::new(),
            token: format!("backfill-{}", uuid::Uuid::new_v4()),
            base: format!("http://{addr}"),
        };
        let messages = fetch_since(&client, "20", "100").await.unwrap();
        assert_eq!(messages.len(), 200);
        assert_eq!(text(messages.first().unwrap(), "id"), "101");
        assert_eq!(text(messages.last().unwrap(), "id"), "300");
        server.abort();
    }

    #[tokio::test]
    async fn in_flight_delivery_never_repeats_model_or_external_write() {
        let (s, dir) = state();
        let mut c = config();
        c["driver"] = json!("dominion-discord");
        c["enabled"] = json!(true);
        c["revision"] = json!("r1");
        s.store.put("channel", &c);
        s.save_secret(&token_name("c"), "not-a-live-token");
        let job = json!({"id":"job","kind":"discord","status":"running","input":{"connectionId":"c","configVersion":"r1","botId":"40","message":message()}});
        s.store.put("job", &job);
        s.store.put(
            "discord_delivery",
            &json!({"id":"job:a","status":"sending","content":"saved response","sent":0}),
        );
        let context = JobContext::new(s.clone(), "job".into());
        let error = run(&context, &job).await.unwrap_err();
        assert!(error.starts_with("needs_review:"));
        assert_eq!(s.store.count("execution"), 0);
        drop(context);
        drop(s);
        let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn legacy_bindings_and_invalid_credentials_do_not_auto_connect() {
        let mut c = config();
        c["enabled"] = json!(true);
        assert!(!native_enabled(&c));
        c["driver"] = json!("dominion-discord");
        assert!(native_enabled(&c));
        c["runtimeError"] = json!("Discord HTTP 401: invalid token");
        assert!(!native_enabled(&c));
        c["runtimeError"] = Value::Null;
        assert!(native_enabled(&c));
        c["enabled"] = json!(false);
        assert!(!native_enabled(&c));
    }
    #[tokio::test]
    async fn cancelled_sessions_cannot_post_even_after_generation() {
        let (s, dir) = state();
        let mut c = config();
        c["enabled"] = json!(true);
        c["revision"] = json!("r1");
        s.store.put("channel", &c);
        s.cancel_session("cancelled-session");
        let client = DiscordClient {
            client: reqwest::Client::new(),
            token: "cancel-test".into(),
            base: "http://127.0.0.1:1".into(),
        };
        let mut delivery = json!({"id":"cancelled-delivery","sessionId":"cancelled-session","status":"ready","content":"generated","sent":0});
        let error = deliver(
            &s,
            &client,
            &c,
            &message(),
            &json!({"name":"가람"}),
            &mut delivery,
        )
        .await
        .unwrap_err();
        assert!(error.starts_with("cancelled:"));
        assert_eq!(delivery["status"], "ready");
        drop(s);
        let _ = std::fs::remove_dir_all(dir);
    }
    #[tokio::test]
    async fn directed_bot_mentions_require_explicit_opt_in_and_cannot_ping_others() {
        use axum::{extract::State, routing::post, Json, Router};
        let captured = Arc::new(Mutex::new(Vec::<Value>::new()));
        async fn send(
            State(out): State<Arc<Mutex<Vec<Value>>>>,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            out.lock().unwrap().push(body);
            Json(json!({"id":"200"}))
        }
        let app = Router::new()
            .route("/channels/20/messages", post(send))
            .with_state(captured.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = DiscordClient {
            client: reqwest::Client::new(),
            token: format!("mention-mock-{}", uuid::Uuid::new_v4()),
            base: format!("http://{addr}"),
        };
        let (s, dir) = state();
        let mut c = config();
        c["enabled"] = json!(true);
        c["revision"] = json!("v1");
        c["allowedBotIds"] = json!("30");
        let mut m = message();
        m["author"]["bot"] = json!(true);
        assert_eq!(reply_recipient(&c, &m), None);
        c["replyMentionBots"] = json!(true);
        assert_eq!(reply_recipient(&c, &m), Some("30".into()));
        let mut unapproved = m.clone();
        unapproved["author"]["id"] = json!("31");
        assert_eq!(reply_recipient(&c, &unapproved), None);
        let mut human = m.clone();
        human["author"]["bot"] = json!(false);
        assert_eq!(reply_recipient(&c, &human), None);
        s.store.put("channel", &c);
        let mut delivery = json!({"id":"directed","status":"ready","content":format!("<@999> @everyone {}","답😀".repeat(900)),"sent":0});
        deliver(&s, &client, &c, &m, &json!({"name":"가람"}), &mut delivery)
            .await
            .unwrap();
        let out = captured.lock().unwrap();
        assert_eq!(out.len(), 2);
        assert!(text(&out[0], "content").starts_with("<@30> "));
        assert_eq!(out[0]["allowed_mentions"]["users"], json!(["30"]));
        assert_eq!(out[1]["allowed_mentions"]["users"], json!([]));
        let combined = out
            .iter()
            .map(|p| text(p, "content"))
            .collect::<Vec<_>>()
            .concat();
        assert_eq!(combined.matches("<@30>").count(), 1);
        assert!(!combined.contains("<@999>"));
        assert!(!combined.contains("@everyone"));
        assert!(out
            .iter()
            .all(|p| p["allowed_mentions"]["parse"] == json!([])
                && p["allowed_mentions"]["replied_user"] == false
                && text(p, "content").encode_utf16().count() <= 2000));
        drop(out);
        server.abort();
        drop(s);
        let _ = std::fs::remove_dir_all(dir);
    }
