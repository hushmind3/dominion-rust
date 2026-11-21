//! P3 opt-in remote read-only worker endpoint. Every remote job is a
//! normal native Run in the existing durable repository/harness. No worker
//! owns a second scheduler, tool dispatcher, or authority to modify files.
use crate::{domain::{CreateRun,Run,RunStatus},engine::Engine};
use axum::{extract::{Path,State},http::{HeaderMap,StatusCode},routing::{get,post},Json,Router};
use serde::Deserialize;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::sync::Arc;
use uuid::Uuid;

type Fault=(StatusCode,Json<Value>);
fn denied()->Fault{(StatusCode::UNAUTHORIZED,Json(json!({"error":"remote worker capability required"})))}
fn disabled()->Fault{(StatusCode::NOT_FOUND,Json(json!({"error":"remote workers disabled"})))}
fn worker_token_matches(expected:&str,provided:&str)->bool{
    if expected.len()<32||provided.len()!=expected.len(){return false;}
    let left=Sha256::digest(expected.as_bytes());
    let right=Sha256::digest(provided.as_bytes());
    left.iter().zip(right.iter()).fold(0u8,|diff,(l,r)|diff|(*l^*r))==0
}
fn authorized(headers:&HeaderMap)->Result<(),Fault>{
    let expected=std::env::var("DOMINION_REMOTE_WORKER_ACCEPT_TOKEN")
        .ok().filter(|v|v.len()>=32).ok_or_else(disabled)?;
    let provided=headers.get("x-dominion-worker-token")
        .and_then(|v|v.to_str().ok()).unwrap_or("");
    if worker_token_matches(&expected,provided){Ok(())}else{Err(denied())}
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Request {
    run_id:Uuid,
    instruction:String,
    budget_krw:f64,
    max_steps:usize,
}
fn reply(run:&Run)->Value{
    json!({"id":run.id,"status":run.status,
        "answer":if run.terminal(){run.answer.as_deref()}else{None},
        "costKrw":run.cost_krw,
        "inputTokens":run.input_tokens,"outputTokens":run.output_tokens,
        "pendingApproval":run.status==RunStatus::AwaitingApproval,
        "source":"native_readonly_worker"})
}
async fn submit(State(engine):State<Arc<Engine>>,headers:HeaderMap,
    Json(req):Json<Request>)->Result<(StatusCode,Json<Value>),Fault>{
    authorized(&headers)?;
    if req.instruction.trim().is_empty()||req.instruction.len()>12000
        ||!req.budget_krw.is_finite()||!(0.0..=10000.0).contains(&req.budget_krw)
        ||!(1..=20).contains(&req.max_steps){
        return Err((StatusCode::BAD_REQUEST,Json(json!({"error":"invalid remote worker budget, steps or instruction"}))));
    }
    let mut run=Run::new(CreateRun{instruction:req.instruction.clone(),
        budget_krw:Some(req.budget_krw),max_steps:Some(req.max_steps)},20)
        .map_err(|error|(StatusCode::BAD_REQUEST,Json(json!({"error":error}))))?;
    run.id=req.run_id;
    run.read_only=true;
    run.delegation_profile="inspection".into();
    run.provider_id="dominion-native".into();
    run.model_name=engine.settings.main.name.clone();
    run.event("remote_worker_admitted","Scoped read-only remote Run admitted",
        Some(json!({"protocolVersion":1,"readOnly":true,"remoteWorker":true})));
    let (current,created)=engine.store.put_if_absent(run).await.map_err(|e|
        (StatusCode::INTERNAL_SERVER_ERROR,Json(json!({"error":e}))))?;
    if current.instruction!=req.instruction.trim()
        ||current.budget_krw!=req.budget_krw
        ||current.max_steps!=req.max_steps
        ||!current.read_only||current.delegation_profile!="inspection"
        ||!current.events.iter().any(|event|event.kind=="remote_worker_admitted"){
        return Err((StatusCode::CONFLICT,Json(json!({"error":"remote run ID already belongs to a different instruction or capability"}))));
    }
    if current.status==RunStatus::Queued{
        if let Err(error)=engine.launch(current.id).await{
            // A concurrent retry may already have launched the same Run.
            if !error.contains("already active"){
                return Err((StatusCode::CONFLICT,Json(json!({"error":error}))));
            }
        }
    }
    let current=engine.store.get(current.id).await.unwrap_or(current);
    Ok((if created{StatusCode::CREATED}else{StatusCode::OK},Json(reply(&current))))
}
async fn read(State(engine):State<Arc<Engine>>,headers:HeaderMap,
    Path(id):Path<Uuid>)->Result<Json<Value>,Fault>{
    authorized(&headers)?;
    let run=engine.store.get(id).await.ok_or_else(||
        (StatusCode::NOT_FOUND,Json(json!({"error":"remote execution not found"}))))?;
    if !run.read_only||!run.events.iter().any(|e|e.kind=="remote_worker_admitted"){
        return Err((StatusCode::NOT_FOUND,Json(json!({"error":"remote execution not found"}))));
    }
    Ok(Json(reply(&run)))
}
async fn capabilities(State(engine):State<Arc<Engine>>,headers:HeaderMap)
    ->Result<Json<Value>,Fault>{
    authorized(&headers)?;
    let slots=std::env::var("DOMINION_REMOTE_WORKER_SLOTS").ok()
        .and_then(|v|v.parse::<u64>().ok()).unwrap_or(4).clamp(1,64);
    let active=engine.store.stats().await.active as u64;
    Ok(Json(json!({"schemaVersion":1,"readOnly":true,
        "writer":false,"active":active,"slots":slots})))
}
pub fn routes()->Router<Arc<Engine>>{
    Router::new()
        .route("/remote-worker/capabilities",get(capabilities))
        .route("/remote-worker/runs",post(submit))
        .route("/remote-worker/runs/{id}",get(read))
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn scoped_capability_requires_long_identical_secret(){
        let secret="A-32-byte-plus-remote-worker-token-for-tests";
        assert!(worker_token_matches(secret,secret));
        assert!(!worker_token_matches(secret,"different-32-byte-long-remote-worker-token"));
        assert!(!worker_token_matches("short","short"));
    }
}
