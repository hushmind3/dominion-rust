pub mod goal_control;
pub mod goal_steering;
pub mod goal_git;
pub mod goal_feedback;
pub mod adaptation_loop;
pub mod distill;
pub mod effect;
pub mod agent_growth;
pub mod algorithms;
pub mod algorithm_eval;
pub mod attachments;
pub mod backup_keys;
pub mod bridge;
pub mod chat;
pub mod browser;
pub mod compact;
pub mod channel_adapter;
pub mod decompose;
pub mod debate;
pub mod dev;
pub mod discord;
pub mod adaptation;
pub mod episode;
pub mod experience;
pub mod procedure;
pub mod program;
pub mod verification;
pub mod reflect;
pub mod goals;
pub mod gateway;
pub mod cli_model;
pub mod improve;
pub mod code_index;
pub mod rsi;
#[path = "rsi/lineage/archive.rs"]
pub mod rsi_archive;
#[path = "rsi/evaluation/metrics.rs"]
pub mod rsi_metrics;
pub mod rsi_evolve;
pub mod rsi_system;
pub mod rsi_meta_controller;
pub mod rsi_prequential;
pub mod rsi_recursive_rewrite;
pub mod rsi_principle_learning;
#[path = "rsi/meta/model_ecology.rs"]
pub mod rsi_model_ecology;
#[path = "rsi/learning/credit_assignment.rs"]
pub mod rsi_credit_assignment;
#[path = "rsi/learning/negative_transfer.rs"]
pub mod rsi_negative_transfer;
#[path = "rsi/meta/symbiosis.rs"]
pub mod rsi_symbiosis;
#[path = "rsi/evaluation/intervention_lab.rs"]
pub mod rsi_intervention_lab;
#[path = "rsi/planner/open_topology.rs"]
pub mod rsi_open_topology;
#[path = "rsi/lineage/transfer.rs"]
pub mod rsi_lineage_transfer;
#[path = "rsi/lineage/history.rs"]
pub mod rsi_history;
#[path = "rsi/proof/closure.rs"]
pub mod rsi_closure;
#[path = "rsi/proof/path_policy.rs"]
pub mod rsi_path_policy;
#[path = "rsi/learning/edit_genome.rs"]
pub mod rsi_edit_genome;
#[path = "rsi/learning/edit_program.rs"]
pub mod rsi_edit_program;
#[path = "rsi/meta/decision_rules.rs"]
pub mod rsi_decision_rules;
#[path = "rsi/evaluation/rule_tournament.rs"]
pub mod rsi_rule_tournament;
#[path = "rsi/learning/rule_rewrite.rs"]
pub mod rsi_rule_rewrite;
#[path = "rsi/learning/rule_induction.rs"]
pub mod rsi_rule_induction;
#[path = "rsi/planner/scan.rs"]
pub mod rsi_scan;
#[path = "rsi/evaluation/worktree.rs"]
pub mod rsi_worktree;
#[path = "rsi/execution/compiler_repair.rs"]
pub mod rsi_compiler_repair;
#[path = "rsi/execution/mutation.rs"]
pub mod rsi_mutation;
#[path = "rsi/execution/repair_synthesis.rs"]
pub mod rsi_repair_synthesis;
pub mod recovery;
pub mod reasoning;
pub mod remote_backup;
pub mod jobs;
pub mod learning;
pub mod lease;
pub mod llm;
pub mod mcp;
pub mod maintenance;
pub mod memory;
pub mod models;
pub mod codex_oauth;
pub mod oauth;
pub mod personas;
pub mod plan_quality;
pub mod policy;
pub mod policy_patch;
pub mod preflight_fixture;
pub mod principle;
pub mod router;
pub mod rooms;
pub mod room_knowledge;
pub mod room_state;
pub mod run_state;
pub mod scheduler;
pub mod skills;
pub mod telegram;
pub mod tools;
pub mod unsafe_audit;
pub mod vault_rotation;
pub mod video;
pub mod workflows;
pub mod wake;

use crate::state::AppState;
use serde_json::Value;
use tokio::sync::mpsc;

#[derive(Default, Clone)]
pub struct TurnStats {
    pub tool_success: Vec<String>,
    pub tool_errors: Vec<String>,
    pub evidence: Vec<Value>,
}

/// A transient streamed tool_end message is NOT a goal receipt. Only a
/// completed, persisted execution_operation with matching session, call ID
/// and tool identity can become acceptance evidence. Keep the original
/// runId, opSeq and timestamps for later source-mutation freshness checks.
fn committed_goal_evidence(state:&AppState, session:Option<&str>, events:Vec<Value>)->Vec<Value>{
    events.into_iter().filter_map(|event|{
        let operation_id=event["operationId"].as_str()?;
        let saved=state.store.get("execution_operation",operation_id)?;
        if saved["status"]!="completed" || saved["result"].is_null()
            || saved["tool"]!=event["tool"]
            || saved["callId"]!=event["callId"] {return None;}
        if session.is_some_and(|expected|saved["sessionId"].as_str()!=Some(expected)){
            return None;
        }
        crate::engine::verification::successful_evidence(&saved).then_some(saved)
    }).collect()
}

/// Runs a turn and collects tool evidence for the goal harness.
pub async fn run_quiet_outcome(
    state: &AppState,
    request: &chat::TurnRequest,
) -> (Result<Option<Value>, String>, TurnStats) {
    let (sender, mut receiver) = mpsc::unbounded_channel::<Value>();
    let collector = tokio::spawn(async move {
        let mut stats = TurnStats::default();
        while let Some(event) = receiver.recv().await {
            let name = event
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            match event.get("type").and_then(|value| value.as_str()) {
                Some("tool_end") if !name.is_empty() => {
                    // Store only a pointer until the turn has returned and
                    // the final SQLite execution operation can be verified.
                    stats.evidence.push(serde_json::json!({
                        "tool":name,"callId":event.get("id"),
                        "operationId":event.get("operationId"),
                    }));
                }
                Some("tool_error") if !name.is_empty() => stats.tool_errors.push(name.to_string()),
                _ => {}
            }
        }
        stats
    });
    let result = chat::run_turn_background(state, request, &sender).await;
    drop(sender);
    let mut stats = collector.await.unwrap_or_default();
    stats.evidence=committed_goal_evidence(state,request.session_id.as_deref(),stats.evidence);
    stats.tool_success=stats.evidence.iter()
        .filter_map(|receipt|receipt["tool"].as_str().map(str::to_owned))
        .collect();
    (result, stats)
}

/// Background resume that never records a second owner message.
pub async fn run_quiet_resume(
    state: &AppState,
    request: &chat::TurnRequest,
) -> Result<Option<Value>, String> {
    let (sender, mut receiver) = mpsc::unbounded_channel::<Value>();
    let drain = tokio::spawn(async move {
        while receiver.recv().await.is_some() {}
    });
    let result = chat::run_turn_resume(state, request, &sender).await;
    drop(sender);
    let _ = drain.await;
    result
}

/// Compatibility wrapper; harnesses that need evidence after a model/runtime
/// error use run_quiet_outcome instead of discarding completed tool work.
pub async fn run_quiet_with_stats(
    state: &AppState,
    request: &chat::TurnRequest,
) -> Result<(Option<Value>, TurnStats), String> {
    let (result, stats) = run_quiet_outcome(state, request).await;
    result.map(|message| (message, stats))
}

/// Runs a turn with nobody listening on the event stream (background jobs).
/// The channel stays open so tool execution is not treated as cancelled.
pub async fn run_quiet(
    state: &AppState,
    request: &chat::TurnRequest,
) -> Result<Option<Value>, String> {
    run_quiet_with_stats(state, request)
        .await
        .map(|(message, _)| message)
}

/// A room worker owns generation and persistence independently of open tabs.
/// Forward transient progress to connected UIs; the stored message is truth.
pub async fn run_room_quiet(
    state: &AppState,
    request: &chat::TurnRequest,
    source_message_id: &str,
) -> Result<Option<Value>, String> {
    let session_id = request
        .session_id
        .clone()
        .ok_or("그룹 세션이 필요합니다.")?;
    let session = state
        .store
        .get("session", &session_id)
        .ok_or("그룹 세션을 찾을 수 없습니다.")?;
    let room_id = session["roomId"]
        .as_str()
        .ok_or("그룹 대화 연결이 없습니다.")?
        .to_string();
    let agent_id = request.agent_id.clone();
    let turn_id = uuid::Uuid::new_v4().to_string();
    state.bus.emit(serde_json::json!({"type":"room_event","roomId":room_id,"sessionId":session_id,"agentId":agent_id,"turnId":turn_id,"event":{"type":"turn_start","sourceMessageId":source_message_id}}));
    let bus = state.bus.clone();
    let (sender, mut receiver) = mpsc::unbounded_channel::<Value>();
    let stream_room = room_id.clone();
    let stream_session = session_id.clone();
    let stream_agent = agent_id.clone();
    let stream_turn = turn_id.clone();
    let collector = tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            bus.emit(serde_json::json!({"type":"room_event","roomId":stream_room,"sessionId":stream_session,"agentId":stream_agent,"turnId":stream_turn,"event":event}));
        }
    });
    let result = chat::run_room_turn(state, request, &sender, source_message_id).await;
    drop(sender);
    let _ = collector.await;
    let event = match &result {
        Ok(Some(message)) => {
            serde_json::json!({"type":"assistant_saved","messageId":message["id"]})
        }
        Ok(None) => serde_json::json!({"type":"done"}),
        Err(error) => serde_json::json!({"type":"error","message":error,"terminal":true}),
    };
    state.bus.emit(serde_json::json!({"type":"room_event","roomId":room_id,"sessionId":session_id,"agentId":agent_id,"turnId":turn_id,"event":event}));
    result
}

#[cfg(test)]
mod outcome_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_committed_tool_receipts_can_become_goal_evidence(){
        let root=std::env::temp_dir().join(format!("dominion-receipts-{}",uuid::Uuid::new_v4()));
        let cfg=crate::config::Config{port:0,data_dir:root.clone(),
            web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(cfg.clone(),crate::db::Store::open(&cfg.db_path()).unwrap(),
            crate::vault::Vault::open(&cfg.key_path()));
        let event=json!({"tool":"workspace_write","callId":"call-1","operationId":"op-1"});
        assert!(committed_goal_evidence(&state,Some("s"),vec![event.clone()]).is_empty());
        state.store.put("execution_operation",&json!({
            "id":"op-1","tool":"workspace_write","sessionId":"other",
            "callId":"call-1","status":"completed",
            "runId":"run-1","opSeq":2,"startedAt":"2026-11-14T10:00:00Z",
            "result":{"written":true}})).unwrap();
        assert!(committed_goal_evidence(&state,Some("s"),vec![event.clone()]).is_empty());
        state.store.patch_if("execution_operation","op-1","sessionId",&json!("other"),
            &json!({"sessionId":"s"})).unwrap();
        let evidence=committed_goal_evidence(&state,Some("s"),vec![event.clone()]);
        assert_eq!(evidence.len(),1);
        assert_eq!(evidence[0]["runId"],"run-1");
        assert_eq!(evidence[0]["opSeq"],2);
        assert_eq!(evidence[0]["result"]["written"],true);
        assert!(committed_goal_evidence(&state,Some("s"),
            vec![json!({"tool":"file_write","callId":"call-1",
                "operationId":"op-1"})]).is_empty());
        state.store.patch_if("execution_operation","op-1","status",&json!("completed"),
            &json!({"status":"failed"})).unwrap();
        assert!(committed_goal_evidence(&state,Some("s"),vec![event]).is_empty());
        let _=std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn successful_write_evidence_survives_a_later_provider_failure() {
        use axum::{response::IntoResponse, routing::post, Json, Router};
        async fn provider(Json(body): Json<Value>) -> axum::response::Response {
            if body["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|message| message["role"] == "tool")
            {
                return (
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({"error":"fixture failure after completed write"})),
                )
                    .into_response();
            }
            let event = json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"fixture-write","function":{"name":"workspace_write","arguments":json!({"path":"proof.txt","content":"proof"}).to_string()}}]},"finish_reason":"tool_calls"}]});
            (
                [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                format!("data: {event}\n\ndata: [DONE]\n\n"),
            )
                .into_response()
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route("/chat/completions", post(provider)),
            )
            .await
            .unwrap()
        });
        let root = std::env::temp_dir().join(format!("dominion-outcome-{}", uuid::Uuid::new_v4()));
        let config = crate::config::Config {
            port: 0,
            data_dir: root.clone(),
            web_dir: root.join("web"),
            workspace: root.clone(),
        };
        let state = AppState::new(
            config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()),
        );
        state.store.put(
            "provider",
            &json!({"id":"p","baseUrl":format!("http://{addr}"),"model":"fixture"}),
        );
        state.store.put("session", &json!({"id":"s"}));
        state
            .store
            .put("policy", &json!({"id":"default","autoApproveAll":true}));
        let request=chat::TurnRequest::from_value(&json!({"sessionId":"s","providerId":"p","input":"Write the proof file","autoLearning":false})).unwrap();
        let (outcome, stats) = run_quiet_outcome(&state, &request).await;
        assert!(outcome.unwrap_err().contains("503"));
        assert_eq!(
            std::fs::read_to_string(root.join("proof.txt")).unwrap(),
            "proof"
        );
        assert_eq!(stats.tool_success, vec!["workspace_write"]);
        assert_eq!(stats.evidence.len(), 1);
        assert_eq!(stats.evidence[0]["tool"], "workspace_write");
        assert_eq!(
            state.store.all("execution_operation")[0]["status"],
            "completed"
        );
        server.abort();
        let _ = std::fs::remove_dir_all(root);
    }
}
