use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::sse::{Event, Sse},
    routing::{get, post},
    Json, Router,
};
use dominion_sdk::{
    ChatRequest, DominionClient, DominionError, GoalCreate, ModelSelection,
    SessionCreate, WaitOptions, WatchOptions,
};
use futures_util::{stream, StreamExt};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    convert::Infallible,
    sync::{atomic::{AtomicUsize, Ordering}, Arc},
    time::Duration,
};

async fn serve(app: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{address}"), task)
}

fn goal(id: &str, status: &str) -> Value {
    json!({"id":id,"goal":"Fix a regression","status":status,"jobId":"j1","iterations":1})
}
fn sse(items: Vec<Event>) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    Sse::new(stream::iter(items.into_iter().map(Ok::<_, Infallible>)))
}

#[tokio::test]
async fn rust_goal_api_is_typed_and_authorized_and_can_be_steered() {
    let app = Router::new()
        .route("/api/goals", post(|headers: HeaderMap, Json(body): Json<Value>| async move {
            assert_eq!(headers.get("x-dominion-token").unwrap(), "secret");
            assert_eq!(body["goal"], "Fix a regression");
            assert_eq!(body["maxIterations"], 8);
            assert_eq!(body["executor"]["model"], "rust-model");
            assert_eq!(body["isolateWorktree"], true);
            Json(goal("g1", "queued"))
        }))
        .route("/api/goals/:id", get(|Path(id): Path<String>| async move {
            assert_eq!(id, "g1");
            Json(json!({"id":"g1","goal":"Fix a regression","status":"running","planRevision":4}))
        }))
        .route("/api/goals/:id/steer", post(|Path(id): Path<String>, Json(body): Json<Value>| async move {
            assert_eq!(id, "g1");
            assert_eq!(body["expectedRevision"], 4);
            if let Some(plan) = body["plan"].as_str() {
                assert_eq!(plan, "Read files, patch, then test");
            } else {
                assert_eq!(body["instruction"], "최신 지시대로 실제로 테스트하세요");
            }
            let kind = if body["instruction"].is_string() { "instruction" } else { "plan" };
            Json(json!({"id":"g1","status":"pending_application","kind":kind,"planRevision":5}))
        }))
        .route("/api/goals/:id/cancel", post(|Path(id): Path<String>| async move {
            assert_eq!(id, "g1");
            Json(goal("g1", "cancelled"))
        }))
        .route("/api/goals/:id/resume", post(|Json(body): Json<Value>| async move {
            assert_eq!(body, json!({"acknowledgeUnknownEffects":true}));
            Json(goal("g1", "queued"))
        }));
    let (base, handle) = serve(app).await;
    let sdk = DominionClient::new(&base).unwrap().with_admin_token("secret").unwrap();
    let selection = ModelSelection { model: Some("rust-model".into()), ..Default::default() };
    let created = sdk.create_goal(
        GoalCreate::new("Fix a regression")
            .max_iterations(8)
            .isolate_worktree(true)
            .executor(selection)
    ).await.unwrap();
    assert_eq!(created.id, "g1");
    let goal_handle = sdk.goal_handle(created.id);
    let plan_receipt = goal_handle.steer("Read files, patch, then test").await.unwrap();
    assert_eq!(plan_receipt.status, "pending_application");
    assert_eq!(plan_receipt.kind, "plan");
    let instruction = goal_handle.instruct("최신 지시대로 실제로 테스트하세요").await.unwrap();
    assert_eq!(instruction.kind, "instruction");
    assert_eq!(instruction.plan_revision, 5);
    assert_eq!(goal_handle.cancel().await.unwrap().status, "cancelled");
    assert_eq!(goal_handle.resume(true).await.unwrap().status, "queued");
    handle.abort();
}

#[tokio::test]
async fn rust_goal_wait_polls_durable_goal_without_restarting_execution() {
    let count = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route("/api/goals/:id", get(|State(count): State<Arc<AtomicUsize>>| async move {
            let current = count.fetch_add(1, Ordering::SeqCst);
            Json(goal("g1", if current == 0 { "running" } else { "needs_review" }))
        }))
        .with_state(count.clone());
    let (base, handle) = serve(app).await;
    let sdk = DominionClient::new(&base).unwrap();
    let result = sdk.wait_goal("g1", WaitOptions {
        poll_interval: Duration::from_millis(15),
        timeout: Duration::from_secs(2),
    }).await.unwrap();
    assert_eq!(result.status, "needs_review");
    assert_eq!(count.load(Ordering::SeqCst), 2);
    handle.abort();
}

#[tokio::test]
async fn rust_chat_sse_ends_on_done_and_preserves_unicode() {
    let app = Router::new().route("/api/chat", post(|Json(body): Json<Value>| async move {
        assert_eq!(body["input"], "확인");
        assert_eq!(body["sessionId"], "session-1");
        assert_eq!(body["toolsExecutionEnabled"], true);
        sse(vec![
            Event::default().data(json!({"type":"delta","content":"안녕하세요"}).to_string()),
            Event::default().data(json!({"type":"done"}).to_string()),
        ])
    }));
    let (base, handle) = serve(app).await;
    let sdk = DominionClient::new(&base).unwrap();
    let mut turns = sdk.stream_chat(ChatRequest::new("session-1", "확인")).await.unwrap();
    assert_eq!(turns.next().await.unwrap().unwrap().field("content").unwrap(), "안녕하세요");
    assert_eq!(turns.next().await.unwrap().unwrap().kind, "done");
    assert!(turns.next().await.is_none());
    handle.abort();
}

#[tokio::test]
async fn rust_chat_incomplete_sse_is_error_and_post_is_not_replayed() {
    let posts = Arc::new(AtomicUsize::new(0));
    let app = Router::new().route("/api/chat",
        post(|State(posts): State<Arc<AtomicUsize>>| async move {
            posts.fetch_add(1, Ordering::SeqCst);
            sse(vec![Event::default().data(json!({"type":"delta","content":"partial"}).to_string())])
        })).with_state(posts.clone());
    let (base, handle) = serve(app).await;
    let sdk = DominionClient::new(&base).unwrap();
    let mut turns = sdk.stream_chat(ChatRequest::new("s", "edit")).await.unwrap();
    assert_eq!(turns.next().await.unwrap().unwrap().kind, "delta");
    assert!(matches!(turns.next().await.unwrap(), Err(DominionError::Interrupted)));
    assert_eq!(posts.load(Ordering::SeqCst), 1);
    handle.abort();
}

#[tokio::test]
async fn rust_durable_events_reconnect_by_cursor_and_eliminate_duplicates() {
    let count = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route("/api/events", get(|State(count): State<Arc<AtomicUsize>>,
            Query(query): Query<HashMap<String,String>>| async move {
            let round = count.fetch_add(1, Ordering::SeqCst);
            if round == 0 {
                assert_eq!(query["after"], "0");
                sse(vec![
                    Event::default().id("1").data(json!({"seq":1,"type":"record"}).to_string()),
                    Event::default().id("2").data(json!({"seq":2,"type":"approval"}).to_string()),
                ])
            } else {
                assert_eq!(query["after"], "2");
                sse(vec![
                    Event::default().id("2").data(json!({"seq":2,"type":"approval"}).to_string()),
                    Event::default().id("3").data(json!({"seq":3,"type":"record"}).to_string()),
                ])
            }
        })).with_state(count.clone());
    let (base, handle) = serve(app).await;
    let sdk = DominionClient::new(&base).unwrap();
    let mut events = sdk.watch_events(WatchOptions {
        retry_delay: Duration::from_millis(5),
        max_reconnects: Some(2),
        ..Default::default()
    });
    let one = tokio::time::timeout(Duration::from_secs(2), events.next()).await.unwrap()
        .unwrap().unwrap();
    let two = tokio::time::timeout(Duration::from_secs(2), events.next()).await.unwrap()
        .unwrap().unwrap();
    let three = tokio::time::timeout(Duration::from_secs(2), events.next()).await.unwrap()
        .unwrap().unwrap();
    assert_eq!((one.seq, two.seq, three.seq), (1,2,3));
    assert_eq!(count.load(Ordering::SeqCst), 2);
    handle.abort();
}

#[tokio::test]
async fn rust_rejects_invalid_event_sequences_without_retry_loop() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = Router::new().route("/api/events", get(|State(calls): State<Arc<AtomicUsize>>| async move {
        calls.fetch_add(1, Ordering::SeqCst);
        sse(vec![Event::default().id("9")
            .data(json!({"seq":2,"type":"record"}).to_string())])
    })).with_state(calls.clone());
    let (base, handle) = serve(app).await;
    let sdk = DominionClient::new(&base).unwrap();
    let mut stream = sdk.watch_events(WatchOptions::default());
    assert!(matches!(stream.next().await.unwrap(), Err(DominionError::Protocol(_))));
    assert_eq!(calls.load(Ordering::SeqCst),1);
    handle.abort();
}

#[tokio::test]
async fn rust_http_error_keeps_status_and_does_not_retry_write() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = Router::new().route("/api/goals", post(|State(calls): State<Arc<AtomicUsize>>| async move {
        calls.fetch_add(1, Ordering::SeqCst);
        (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"maintenance"})))
    })).with_state(calls.clone());
    let (base, handle) = serve(app).await;
    let sdk = DominionClient::new(&base).unwrap();
    let error = sdk.create_goal(GoalCreate::new("fix")).await.unwrap_err();
    match error {
        DominionError::Http {status, message} => {
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
            assert!(message.contains("maintenance"));
        }
        other => panic!("unexpected: {other}"),
    }
    assert_eq!(calls.load(Ordering::SeqCst),1);
    handle.abort();
}

#[tokio::test]
async fn rust_thread_uses_persisted_new_answer_not_previous_reply() {
    let reads = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route("/api/sessions", post(|| async {
            Json(json!({"id":"s1","title":"agent"}))
        }))
        .route("/api/sessions/:id/messages",
            get(|State(reads): State<Arc<AtomicUsize>>| async move {
                let current = reads.fetch_add(1, Ordering::SeqCst);
                let mut messages = vec![json!({
                    "id":"old","session_id":"s1","role":"assistant","content":"old"
                })];
                if current > 0 { messages.push(json!({
                    "id":"new","session_id":"s1","role":"assistant","content":"new"
                })); }
                Json(messages)
            }))
        .route("/api/chat", post(|| async {
            sse(vec![Event::default().data(json!({"type":"done"}).to_string())])
        }))
        .with_state(reads.clone());
    let (base, handle) = serve(app).await;
    let sdk = DominionClient::new(&base).unwrap();
    let thread = sdk.start_thread(SessionCreate::default()).await.unwrap();
    let result = thread.run("next").await.unwrap();
    assert_eq!(result.final_response, "new");
    assert_eq!(result.message.unwrap().id,"new");
    assert_eq!(result.events.len(),1);
    handle.abort();
}

#[tokio::test]
async fn rust_url_and_credential_validation() {
    assert!(DominionClient::new("file:///tmp/x").is_err());
    assert!(DominionClient::new("http://username:password@127.0.0.1").is_err());
    assert!(DominionClient::new("http://127.0.0.1/?token=leak").is_err());
    assert!(DominionClient::new("http://127.0.0.1:4317").is_ok());
}


#[tokio::test]
async fn rust_reviewed_diff_requires_original_receipt_before_merge() {
    let app = Router::new()
        .route("/api/goals/:id/changes", get(|| async {Json(json!({"reviewDigest":"digest-123"}))}))
        .route("/api/goals/:id/apply", post(|Json(body): Json<Value>| async move {
            assert_eq!(body, json!({"reviewDigest":"digest-123","acknowledgeReview":true}));
            Json(json!({"applied":true}))
        }))
        .route("/api/goals/:id/discard", post(|| async {Json(json!({"discarded":true}))}));
    let (base, server) = serve(app).await;
    let sdk = DominionClient::new(&base).unwrap();
    let goal = sdk.goal_handle("g1");
    assert_eq!(goal.changes().await.unwrap()["reviewDigest"], "digest-123");
    assert!(goal.apply_changes(" ").await.is_err());
    assert_eq!(goal.apply_changes("digest-123").await.unwrap()["applied"],true);
    assert_eq!(goal.discard_changes().await.unwrap()["discarded"],true);
    server.abort();
}

#[tokio::test]
async fn rust_stale_owner_revision_is_rejected_without_replay() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route("/api/goals/:id/steer", post(|State(calls): State<Arc<AtomicUsize>>| async move {
            calls.fetch_add(1, Ordering::SeqCst);
            (StatusCode::CONFLICT, Json(json!({"error":"stale owner revision"})))
        }))
        .with_state(calls.clone());
    let (base, server) = serve(app).await;
    let sdk = DominionClient::new(&base).unwrap();
    let error = sdk.instruct_goal("g1","owner command",3).await.unwrap_err();
    assert!(matches!(error, DominionError::Http{status:StatusCode::CONFLICT,..}));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    server.abort();
}
