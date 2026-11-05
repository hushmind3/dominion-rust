//! Contract tests exercise the public turn path and local provider fixtures.
use super::{
    context::history,
    protocol::SseBuffer,
    provider::{apply_reasoning_options, select_provider},
    runtime::{finalize_execution, tool_succeeded},
};
use crate::state::AppState;
use serde_json::{json, Value};
use std::time::Duration;

use super::*;

fn temp_state() -> AppState {
    let root = std::env::temp_dir().join(format!("dominion-chat-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let config = crate::config::Config {
        port: 0,
        data_dir: root.clone(),
        web_dir: root.join("web"),
        workspace: root,
    };
    let store = crate::db::Store::open(&config.db_path()).unwrap();
    let vault = crate::vault::Vault::open(&config.key_path());
    AppState::new(config, store, vault)
}

#[test]
fn split_korean_utf8_and_crlf_events_are_lossless() {
    let bytes = "data: {\"text\":\"장기기억\"}\r\n\r\ndata: [DONE]\n\n".as_bytes();
    let mut buffer = SseBuffer::default();
    let mut events = Vec::new();
    for byte in bytes {
        buffer.push(&[*byte]).unwrap();
        while let Some(event) = buffer.pop().unwrap() {
            events.push(event);
        }
    }
    assert_eq!(
        events,
        vec!["data: {\"text\":\"장기기억\"}\r\n\r\n", "data: [DONE]\n\n"]
    );
}

#[test]
fn deepseek_controls_use_its_actual_protocol_and_never_send_none_effort() {
    for (thinking, effort, expected) in [
        (Some(false), Some("high"), "disabled"),
        (None, Some("none"), "disabled"),
        (Some(true), Some("minimal"), "enabled"),
    ] {
        let mut body = json!({});
        apply_reasoning_options(&mut body, "deepseek", thinking, effort);
        assert_eq!(body["thinking"]["type"], expected);
        if expected == "disabled" {
            assert!(body.get("reasoning_effort").is_none());
        } else {
            assert_eq!(body["reasoning_effort"], "low");
        }
        assert!(body.get("chat_template_kwargs").is_none());
    }
    let mut body = json!({});
    apply_reasoning_options(&mut body, "deepseek", Some(true), Some("medium"));
    assert_eq!(body["reasoning_effort"], "high");
}

#[tokio::test]
async fn real_chat_request_has_bound_image_common_prompt_native_reasoning_and_timing() {
    use axum::{extract::State, routing::post, Json, Router};
    use base64::Engine;
    let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    async fn model(
        State(captured): State<std::sync::Arc<std::sync::Mutex<Vec<Value>>>>,
        Json(body): Json<Value>,
    ) -> axum::response::Response {
        captured.lock().unwrap().push(body);
        use axum::response::IntoResponse;
        let events = format!(
            "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            json!({"choices":[{"delta":{"content":"fixture-answer","reasoning_content":"This is the original provider reasoning in English, retained without a hidden translation request."}}]}),
            json!({"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"total_tokens":17}})
        );
        (
            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
            events,
        )
            .into_response()
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/chat/completions", post(model))
        .with_state(captured.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let state = temp_state();
    state.store.put("provider",&json!({"id":"p","baseUrl":format!("http://{addr}"),"model":"deepseek-fixture","enabled":true}));
    state
        .store
        .put("session", &json!({"id":"s","providerId":"p"}));
    state.store.put(
        "assistant_settings",
        &json!({"id":"global","instructions":"공통 지침 fixture-global","enabled":true}),
    );
    let file=crate::engine::attachments::upload(&state,&json!({"sessionId":"s","name":"photo.png","mimeType":"image/png","data":base64::engine::general_purpose::STANDARD.encode(b"\x89PNG\r\n\x1a\nfixture")})).unwrap();
    let request=TurnRequest::from_value(&json!({"sessionId":"s","input":"사진을 확인해","attachmentIds":[file["id"]],"toolsEnabled":false,"thinking":false,"reasoningEffort":"none","reasoningLanguage":"ko","autoLearning":false})).unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let saved = tokio::time::timeout(Duration::from_secs(3), run_turn(&state, &request, &tx))
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(saved["reasoning"]
        .as_str()
        .unwrap()
        .contains("original provider reasoning"));
    let requests = captured.lock().unwrap();
    assert_eq!(requests.len(), 1);
    let body = &requests[0];
    assert_eq!(body["thinking"]["type"], "disabled");
    assert!(body.get("reasoning_effort").is_none());
    assert!(body["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("fixture-global"));
    let owner = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["role"] == "user")
        .unwrap();
    assert_eq!(owner["content"][1]["type"], "image_url");
    assert!(owner["content"][1]["image_url"]["url"]
        .as_str()
        .unwrap()
        .starts_with("data:image/png;base64,"));
    let user = state
        .store
        .messages("s")
        .into_iter()
        .find(|message| message["role"] == "user")
        .unwrap();
    assert_eq!(user["metadata"]["attachments"][0]["id"], file["id"]);
    let execution = state.store.all("execution").pop().unwrap();
    assert_eq!(execution["tokens"], 17);
    assert!(execution["firstTokenMs"].is_u64());
    assert!(execution["modelElapsedMs"].is_u64());
    server.abort();
    let _ = std::fs::remove_dir_all(&state.config.data_dir);
}

#[test]
fn failed_commands_signals_and_mcp_errors_are_not_successes() {
    assert!(!tool_succeeded(
        &json!({"exitCode":7,"stdout":"claimed success"})
    ));
    assert!(!tool_succeeded(&json!({"exitCode":null})));
    assert!(!tool_succeeded(&json!({"isError":true,"content":[]})));
    assert!(!tool_succeeded(&json!({"status":403})));
    assert!(tool_succeeded(&json!({"exitCode":0})));
}

#[test]
fn cancelled_execution_finalizes_unknown_tool_outcomes() {
    let state = temp_state();
    state
        .store
        .put("execution", &json!({"id":"e","status":"running"}));
    state.store.put(
        "execution_operation",
        &json!({"id":"op","runId":"e","status":"running","tool":"shell_exec"}),
    );
    finalize_execution(
        &state,
        "e",
        &Err("실행이 중단되었습니다.".into()),
        &std::sync::Mutex::new(Metrics::default()),
    );
    assert_eq!(
        state.store.get("execution", "e").unwrap()["status"],
        "cancelled"
    );
    let operation = state.store.get("execution_operation", "op").unwrap();
    assert_eq!(operation["status"], "interrupted");
    assert_eq!(operation["outcomeUnknown"], true);
    let _ = std::fs::remove_dir_all(&state.config.data_dir);
}

#[test]
fn continuation_keeps_the_selected_model_only_for_the_same_provider_and_agent() {
    let state = temp_state();
    for id in ["p", "other"] {
        state.store.put("provider",&json!({"id":id,"baseUrl":"http://127.0.0.1:1/v1","model":"provider-default","enabled":true}));
    }
    let session = json!({"providerId":"p","model":"my-model","agentId":null});
    let mut request = TurnRequest::from_value(&json!({"input":"continue"})).unwrap();
    assert_eq!(
        select_provider(&state, &request, &session).unwrap().model,
        "my-model"
    );
    request.provider_id = Some("other".into());
    assert_eq!(
        select_provider(&state, &request, &session).unwrap().model,
        "provider-default"
    );
    request.model = Some("explicit".into());
    assert_eq!(
        select_provider(&state, &request, &session).unwrap().model,
        "explicit"
    );
    let _ = std::fs::remove_dir_all(&state.config.data_dir);
}

#[test]
fn room_history_keeps_speakers_distinct_and_drops_orphan_tool_results() {
    let state = temp_state();
    state.store.put("agent", &json!({"id":"a","name":"설계자"}));
    state.store.put("agent", &json!({"id":"b","name":"검토자"}));
    for message in [
        json!({"role":"tool","agent_id":"b","tool_call_id":"orphan","content":"orphan secret"}),
        json!({"role":"assistant","agent_id":"a","content":"설계 의견"}),
        json!({"role":"assistant","agent_id":"b","content":"검토 의견"}),
        json!({"role":"assistant","agent_id":"b","content":"검증하겠습니다","tool_calls":[{"id":"c1","type":"function","function":{"name":"workspace_list","arguments":"{}"}}]}),
        json!({"role":"tool","agent_id":"b","content":"{}","tool_call_id":"c1"}),
        json!({"role":"assistant","agent_id":"b","content":"중단됨","tool_calls":[{"id":"missing"}]}),
    ] {
        let mut message = message;
        message["session_id"] = json!("room");
        state.store.write_message(&message).unwrap();
    }
    let messages = history(&state, "room", Some("b"));
    assert_eq!(messages[0]["role"], "user");
    assert!(messages[0]["content"].as_str().unwrap().contains("설계자"));
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(
        messages.iter().filter(|row| row["role"] == "tool").count(),
        1
    );
    assert!(messages.last().unwrap().get("tool_calls").is_none());
    assert!(!json!(messages).to_string().contains("orphan secret"));
    let _ = std::fs::remove_dir_all(&state.config.data_dir);
}

#[test]
fn parses_xml_style_tool_calls() {
    let (calls, cleaned) = parse_text_tool_calls(
            "확인하겠습니다.\n<tool_call>{\"name\":\"workspace_read\",\"arguments\":{\"path\":\"a.txt\"}}</tool_call>",
        );
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "workspace_read");
    assert!(calls[0].1.contains("a.txt"));
    assert!(!cleaned.contains("tool_call"));
}

#[test]
fn parses_fenced_json_tool_calls() {
    let (calls, _) = parse_text_tool_calls(
        "```json\n{\"tool\":\"shell_exec\",\"args\":{\"command\":\"ls\"}}\n```",
    );
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "shell_exec");
}

#[test]
fn ignores_normal_text() {
    let (calls, cleaned) = parse_text_tool_calls("그냥 일반 답변입니다.");
    assert!(calls.is_empty());
    assert_eq!(cleaned, "그냥 일반 답변입니다.");
}

#[test]
fn concurrent_tool_evidence_statistics_do_not_lose_worker_updates() {
    let state = temp_state();
    std::thread::scope(|scope| {
        for index in 0..8 {
            let store = state.store.clone();
            scope.spawn(move || {
                for _ in 0..50 {
                    store
                        .increment_counters(
                            "tool_stat",
                            "fixture_tool",
                            &[
                                "attempts",
                                if index % 2 == 0 {
                                    "successes"
                                } else {
                                    "failures"
                                },
                            ],
                        )
                        .unwrap();
                }
            });
        }
    });
    let stat = state.store.get("tool_stat", "fixture_tool").unwrap();
    assert_eq!(stat["attempts"], 400);
    assert_eq!(stat["successes"], 200);
    assert_eq!(stat["failures"], 200);
    let _ = std::fs::remove_dir_all(&state.config.data_dir);
}

#[test]
fn parses_bare_json_and_stringified_arguments_for_small_models() {
    let known = ["workspace_write".to_string()]
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    let (calls, cleaned) = super::protocol::parse_text_tool_calls_known(
        "{\"tool\":\"workspace_write\",\"args\":{\"path\":\"a.txt\",\"content\":\"x\"}}",
        &known,
    );
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "workspace_write");
    assert!(calls[0].1.contains("a.txt"));
    assert!(cleaned.is_empty());

    let (calls, _) = super::protocol::parse_text_tool_calls_known(
        "{\"name\":\"workspace_write\",\"arguments\":\"{\\\"path\\\":\\\"b.txt\\\",\\\"content\\\":\\\"y\\\"}\"}",
        &known,
    );
    assert_eq!(calls.len(), 1);
    assert!(calls[0].1.contains("b.txt"));
}

#[test]
fn parses_function_wrapper_and_missing_closing_tag() {
    let known = ["workspace_read".to_string()]
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    let (calls, _) = super::protocol::parse_text_tool_calls_known(
        "<tool_call>{\"function\":{\"name\":\"workspace_read\",\"arguments\":{\"path\":\"a.txt\"}}}",
        &known,
    );
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "workspace_read");
    assert!(calls[0].1.contains("a.txt"));
}
