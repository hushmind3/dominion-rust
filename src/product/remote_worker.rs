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
    #[serde(default)]writer:bool,
    #[serde(default)]repository:Option<String>,
    #[serde(default)]base_commit:Option<String>,
    #[serde(default)]seed_patch:String,
    #[serde(default)]seed_sha256:String,
}
fn writer_enabled()->bool{
    std::env::var("DOMINION_REMOTE_WORKER_ENABLE_WRITER")
        .ok().as_deref()==Some("true")
        &&std::env::var("DOMINION_REMOTE_WORKER_WRITER_TOKEN")
            .ok().is_some_and(|s|s.len()>=32)
}
fn writer_authorized(headers:&HeaderMap)->Result<(),Fault>{
    if !writer_enabled(){return Err(disabled())}
    let expected=std::env::var("DOMINION_REMOTE_WORKER_WRITER_TOKEN")
        .map_err(|_|disabled())?;
    let provided=headers.get("x-dominion-worker-writer-token")
        .and_then(|value|value.to_str().ok()).unwrap_or("");
    if worker_token_matches(&expected,provided){Ok(())}else{Err(denied())}
}
fn writer_repo_allowed(repo:&str)->bool{
    !repo.is_empty()&&repo.len()<=160
        &&std::env::var("DOMINION_REMOTE_WRITER_REPOSITORIES")
            .unwrap_or_default().split(',').map(str::trim)
            .any(|allowed|!allowed.is_empty()&&allowed==repo)
}
fn request_fingerprint(req:&Request)->Result<String,Fault>{
    let payload=serde_json::to_vec(&json!({
        "runId":req.run_id,"instruction":req.instruction,
        "budgetKrw":req.budget_krw,"maxSteps":req.max_steps,
        "writer":req.writer,"repository":req.repository,
        "baseCommit":req.base_commit,"seedPatch":req.seed_patch,
        "seedSha256":req.seed_sha256
    })).map_err(|e|(StatusCode::BAD_REQUEST,Json(json!({"error":e.to_string()}))))?;
    Ok(format!("{:x}",Sha256::digest(payload)))
}
async fn reply(engine:&Engine,run:&Run)->Result<Value,Fault>{
    let writer=run.events.iter().any(|e|
        e.kind=="remote_worker_admitted"
            &&e.detail.as_ref().is_some_and(|v|v["writer"]==true));
    let mut response=json!({"id":run.id,"status":run.status,
        "answer":if run.terminal(){run.answer.as_deref()}else{None},
        "costKrw":run.cost_krw,
        "inputTokens":run.input_tokens,"outputTokens":run.output_tokens,
        "pendingApproval":run.status==RunStatus::AwaitingApproval,
        "writer":writer,
        "source":"native_remote_worker"});
    if writer&&run.status==RunStatus::Completed{
        let seed=run.base_commit.as_deref().ok_or_else(||(
            StatusCode::CONFLICT,Json(json!({"error":"remote writer lost seed revision"}))))?;
        let (patch,digest,paths)=crate::workspace::export_remote_writer_patch(
            &engine.settings,run.id,seed).await.map_err(|error|(
            StatusCode::CONFLICT,Json(json!({"error":format!(
                "remote writer patch review unavailable: {error}")}))))?;
        response["patchSha256"]=json!(digest);
        response["patch"]=json!(patch);
        response["changedPaths"]=json!(paths);
    }
    Ok(response)
}
async fn submit(State(engine):State<Arc<Engine>>,headers:HeaderMap,
    Json(req):Json<Request>)->Result<(StatusCode,Json<Value>),Fault>{
    authorized(&headers)?;
    if req.instruction.trim().is_empty()||req.instruction.len()>12000
        ||!req.budget_krw.is_finite()||!(0.0..=10000.0).contains(&req.budget_krw)
        ||!(1..=20).contains(&req.max_steps){
        return Err((StatusCode::BAD_REQUEST,Json(json!({"error":"invalid remote worker budget, steps or instruction"}))));
    }
    if req.writer{writer_authorized(&headers)?;}
    if req.writer{
        let repo=req.repository.as_deref().unwrap_or("");
        let base=req.base_commit.as_deref().unwrap_or("");
        if !writer_repo_allowed(repo)||base.len()!=40||
            !base.bytes().all(|v|v.is_ascii_hexdigit())||
            req.seed_patch.len()>131_072||
            format!("{:x}",Sha256::digest(req.seed_patch.as_bytes()))!=req.seed_sha256{
            return Err((StatusCode::FORBIDDEN,Json(json!({
                "error":"remote Writer repository, source revision or seed not allowed"
            }))));
        }
    }else if req.repository.is_some()||req.base_commit.is_some()
        ||!req.seed_patch.is_empty()||!req.seed_sha256.is_empty(){
        return Err((StatusCode::BAD_REQUEST,Json(json!({
            "error":"read-only worker cannot request repository write capabilities"
        }))));
    }
    let request_hash=request_fingerprint(&req)?;
    let mut run=Run::new(CreateRun{instruction:req.instruction.clone(),
        budget_krw:Some(req.budget_krw),max_steps:Some(req.max_steps)},20)
        .map_err(|error|(StatusCode::BAD_REQUEST,Json(json!({"error":error}))))?;
    run.id=req.run_id;
    run.read_only=!req.writer;
    run.delegation_profile=if req.writer{"implementation"}else{"inspection"}.into();
    if req.writer{
        run.repository=req.repository.clone();
        run.base_commit=req.base_commit.clone();
    }
    run.provider_id="dominion-native".into();
    run.model_name=engine.settings.main.name.clone();
    run.event("remote_worker_admitted","Scoped read-only remote Run admitted",
        Some(json!({"protocolVersion":2,"readOnly":!req.writer,
            "remoteWorker":true,"writer":req.writer,
            "requestHash":request_hash})));
    let (current,created)=engine.store.put_if_absent(run).await.map_err(|e|
        (StatusCode::INTERNAL_SERVER_ERROR,Json(json!({"error":e}))))?;
    if current.instruction!=req.instruction.trim()
        ||current.budget_krw!=req.budget_krw
        ||current.max_steps!=req.max_steps
        ||current.read_only==req.writer
        ||current.delegation_profile!=if req.writer{"implementation"}else{"inspection"}
        ||current.repository!=req.repository
        ||current.events.iter().find(|e|e.kind=="remote_worker_admitted")
            .and_then(|event|event.detail.as_ref())
            .is_none_or(|detail|detail["requestHash"]!=request_hash){
        return Err((StatusCode::CONFLICT,Json(json!({"error":"remote run ID already belongs to a different instruction or capability"}))));
    }
    if req.writer&&current.status==RunStatus::Queued{
        // A node-local gate serializes the worktree attach + seed commit +
        // native Run launch. Restart after a partially installed tree
        // requires reconciliation; it must never silently execute twice.
        static WRITER_ADMISSION:std::sync::OnceLock<tokio::sync::Mutex<()>>=
            std::sync::OnceLock::new();
        let _gate=WRITER_ADMISSION.get_or_init(||tokio::sync::Mutex::new(()))
            .lock().await;
        let now=engine.store.get(current.id).await.unwrap_or(current.clone());
        if !now.events.iter().any(|e|e.kind=="remote_writer_workspace_ready"){
            if !created{
                return Err((StatusCode::CONFLICT,Json(json!({
                    "error":"writer setup interrupted; reconcile isolated worktree"
                }))));
            }
            let repository=req.repository.as_deref().unwrap();
            let source=req.base_commit.as_deref().unwrap();
            let prepared=async {
                crate::workspace::attach_at(&engine.settings,current.id,
                    repository,Some(source)).await?;
                crate::workspace::install_remote_seed(&engine.settings,
                    current.id,source,&req.seed_patch,&req.seed_sha256).await
            }.await;
            match prepared{
                Ok(seed)=>{
                    engine.store.mutate(current.id,|run|{
                        run.base_commit=Some(seed.clone());
                        run.event("remote_writer_workspace_ready",
                            "Remote Writer isolated detached checkout and seed installed",
                            Some(json!({"baseCommit":source,"workerSeedCommit":seed,
                                "seedSha256":req.seed_sha256})));Ok(())
                    }).await.map_err(|error|(StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error":error}))))?;
                }
                Err(error)=>{
                    let _=engine.store.mutate(current.id,|run|{
                        run.status=RunStatus::Paused;
                        run.event("remote_worker_setup_failed",error.clone(),None);Ok(())
                    }).await;
                    return Err((StatusCode::CONFLICT,Json(json!({
                        "error":"remote Writer checkout failed; manual reconciliation required"
                    }))));
                }
            }
        }
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
    Ok((if created{StatusCode::CREATED}else{StatusCode::OK},
        Json(reply(&engine,&current).await?)))
}
async fn read(State(engine):State<Arc<Engine>>,headers:HeaderMap,
    Path(id):Path<Uuid>)->Result<Json<Value>,Fault>{
    authorized(&headers)?;
    let run=engine.store.get(id).await.ok_or_else(||
        (StatusCode::NOT_FOUND,Json(json!({"error":"remote execution not found"}))))?;
    if !run.events.iter().any(|e|e.kind=="remote_worker_admitted"){
        return Err((StatusCode::NOT_FOUND,Json(json!({"error":"remote execution not found"}))));
    }
    let writer=run.events.iter().any(|e|e.kind=="remote_worker_admitted"
        &&e.detail.as_ref().is_some_and(|v|v["writer"]==true));
    if writer{writer_authorized(&headers)?;}
    Ok(Json(reply(&engine,&run).await?))
}
async fn cancel(State(engine):State<Arc<Engine>>,headers:HeaderMap,
    Path(id):Path<Uuid>)->Result<Json<Value>,Fault>{
    authorized(&headers)?;
    let run=engine.store.get(id).await.ok_or_else(||(
        StatusCode::NOT_FOUND,Json(json!({"error":"remote Run not found"}))))?;
    if !run.events.iter().any(|e|e.kind=="remote_worker_admitted"){
        return Err((StatusCode::NOT_FOUND,Json(json!({"error":"not a remote Run"}))));
    }
    let writer=run.events.iter().any(|e|e.kind=="remote_worker_admitted"
        &&e.detail.as_ref().is_some_and(|v|v["writer"]==true));
    if writer{writer_authorized(&headers)?;}
    let cancelled=engine.cancel(id).await.map_err(|error|(
        StatusCode::CONFLICT,Json(json!({"error":error}))))?;
    Ok(Json(json!({"id":id,"status":cancelled.status,
        "cancelRequested":true})))
}
async fn capabilities(State(engine):State<Arc<Engine>>,headers:HeaderMap)
    ->Result<Json<Value>,Fault>{
    authorized(&headers)?;
    let slots=std::env::var("DOMINION_REMOTE_WORKER_SLOTS").ok()
        .and_then(|v|v.parse::<u64>().ok()).unwrap_or(4).clamp(1,64);
    let active=engine.store.stats().await.active as u64;
    let repos=if writer_enabled(){
        std::env::var("DOMINION_REMOTE_WRITER_REPOSITORIES")
            .unwrap_or_default().split(',').map(str::trim)
            .filter(|v|!v.is_empty()).take(32)
            .map(str::to_owned).collect::<Vec<_>>()
    }else{Vec::new()};
    Ok(Json(json!({"schemaVersion":1,"readOnly":true,
        "writer":writer_enabled(),"writerRepositories":repos,
        "active":active,"slots":slots})))
}
pub fn routes()->Router<Arc<Engine>>{
    Router::new()
        .route("/remote-worker/capabilities",get(capabilities))
        .route("/remote-worker/runs",post(submit))
        .route("/remote-worker/runs/{id}",get(read))
        .route("/remote-worker/runs/{id}/cancel",post(cancel))
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
