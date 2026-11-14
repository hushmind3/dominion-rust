//! Local HTTP/SSE surface for the native Rust SDK.
//! Routes reuse the same AppState and execution engine as /api/chat.
use crate::{engine::agent_sdk::DominionSdk,state::AppState};
use axum::{
    extract::{Path,State},
    http::StatusCode,
    response::{IntoResponse,Response,sse::{Event,KeepAlive,Sse}},
    Json,
};
use serde_json::{json,Value};
use std::convert::Infallible;

fn bad(message:String)->Response {
    (StatusCode::BAD_REQUEST,Json(json!({"error":message}))).into_response()
}

/// Stable, read-only discovery contract for Rust and TypeScript SDK clients.
/// Reports supported *native* operations, never claims external model health.
pub async fn info()->Json<Value>{
    Json(json!({
        "protocol":"dominion-native-agent",
        "apiVersion":1,
        "engine":"rust",
        "transports":["http_json","sse"],
        "capabilities":[
            "durable_goals","goal_worktree","goal_acceptance",
            "goal_steering","review_resume","native_threads",
            "thread_steering","thread_cancellation","event_replay",
            "mcp_tools","rsi_independent_evaluation"
        ],
        "semantics":{
            "runPostIdempotent":false,
            "disconnectCancelsExecution":false,
            "eventReplayGetIdempotent":true,
            "externalToolApproval":"engine_enforced"
        }
    }))
}

pub async fn create(State(state):State<AppState>,Json(body):Json<Value>)->Response {
    match DominionSdk::new(state).start_thread(&body){
        Ok(session)=>Json(session).into_response(),
        Err(err)=>bad(err)
    }
}

pub async fn resume(State(state):State<AppState>,Path(id):Path<String>)->Response {
    match DominionSdk::new(state).resume_thread(&id){
        Ok(session)=>Json(session).into_response(),
        Err(err)=>bad(err)
    }
}

pub async fn history(State(state):State<AppState>,Path(id):Path<String>)->Response {
    match DominionSdk::new(state).history(&id){
        Ok(items)=>Json(json!(items)).into_response(),
        Err(err)=>bad(err)
    }
}

pub async fn stream(
    State(state):State<AppState>,
    Path(id):Path<String>,
    Json(body):Json<Value>
)->Response {
    let mut receiver=match DominionSdk::new(state).run_streamed(&id,&body){
        Ok(receiver)=>receiver,Err(err)=>return bad(err),
    };
    let events=async_stream::stream!{
        while let Some(event)=receiver.recv().await{
            yield Ok::<_,Infallible>(Event::default().data(event.to_string()));
        }
    };
    Sse::new(events).keep_alive(KeepAlive::default()).into_response()
}

pub async fn steer(
    State(state):State<AppState>,
    Path(id):Path<String>,
    Json(body):Json<Value>
)->Response {
    let Some(instruction)=body["instruction"].as_str() else {
        return bad("instruction 문자열이 필요합니다.".into());
    };
    match DominionSdk::new(state).steer(&id,instruction){
        Ok(record)=>(StatusCode::ACCEPTED,Json(record)).into_response(),
        Err(err)=>bad(err),
    }
}

pub async fn cancel(State(state):State<AppState>,Path(id):Path<String>)->Response {
    match DominionSdk::new(state).cancel(&id){
        Ok(())=>Json(json!({"cancelled":true,"sessionId":id})).into_response(),
        Err(err)=>bad(err),
    }
}
