//! Original main DeveloperPage routes, backed by verified local workspace
//! and explicit subprocess jobs. Never use shell interpretation of UI data.
use crate::{developer::{Developer,SourceConfig},engine::Engine};
use axum::{extract::{Path,State},http::StatusCode,routing::{get,post},Json,Router};
use serde_json::{json,Value};
use std::sync::Arc;
use uuid::Uuid;

type Fault=(StatusCode,Json<Value>);
fn invalid(message:impl Into<String>)->Fault{
 (StatusCode::BAD_REQUEST,Json(json!({"error":message.into()})))
}
fn unavailable(message:&str)->Fault{
 (StatusCode::NOT_IMPLEMENTED,Json(json!({"error":message})))
}
pub fn routes()->Router<Arc<Engine>>{
 Router::new()
  .route("/dev/status",get(status))
  .route("/dev/config",get(config).post(save_config))
  .route("/dev/connect",post(connect))
  .route("/dev/diff",get(diff))
  .route("/dev/jobs/{id}",get(job))
  .route("/dev/update",get(update_status).post(update_unavailable))
  .route("/dev/preview/start",post(preview_start))
  .route("/dev/preview/stop",post(preview_stop))
  .route("/dev/{kind}",post(start))
  .route("/system/pick-folder",post(pick_folder))
}
async fn status(State(engine):State<Arc<Engine>>)->Json<Value>{
 Json(engine.developer.status().await)
}
async fn config(State(engine):State<Arc<Engine>>)->Json<SourceConfig>{
 Json(engine.developer.get_config().await)
}
async fn save_config(State(engine):State<Arc<Engine>>,Json(input):Json<Value>)
 ->Result<Json<Value>,Fault>{
 let mut cfg=engine.developer.get_config().await;
 if let Some(v)=input.get("sourceDir").and_then(Value::as_str){cfg.source_dir=v.into();}
 if let Some(v)=input.get("repoUrl").and_then(Value::as_str){cfg.repo_url=v.into();}
 if let Some(v)=input.get("branch").and_then(Value::as_str){cfg.branch=v.into();}
 if let Some(port)=input.get("previewPort").and_then(Value::as_u64){
   cfg.preview_port=u16::try_from(port).map_err(|_|invalid("preview port outside valid range"))?;
 }
 engine.developer.save_config(cfg).await
  .map(|result|Json(json!(result))).map_err(invalid)
}
async fn connect(State(engine):State<Arc<Engine>>,Json(body):Json<Value>)
 ->Result<Json<Value>,Fault>{
 let input=body.as_object().ok_or_else(||invalid("source configuration required"))?;
 let mut cfg=engine.developer.get_config().await;
 if let Some(v)=input.get("sourceDir").and_then(Value::as_str){cfg.source_dir=v.into();}
 if let Some(v)=input.get("repoUrl").and_then(Value::as_str){cfg.repo_url=v.into();}
 if let Some(v)=input.get("branch").and_then(Value::as_str){cfg.branch=v.into();}
 let old=engine.developer.get_config().await;
 engine.developer.save_config(cfg).await.map_err(invalid)?;
 match engine.developer.repository().await{
  Ok(path)=>{
   // Source is already cloned and verified; no network credential prompts
   // and no silent checkout of an unrelated branch.
   Ok(Json(json!({"connected":true,"cloned":false,"sourceDir":path,"git":true})))
  },
  Err(error)=>{
   let _=engine.developer.save_config(old).await;
   Err(invalid(error))
  }
 }
}
async fn diff(State(engine):State<Arc<Engine>>)->Result<Json<Value>,Fault>{
 engine.developer.diff().await.map(Json).map_err(invalid)
}
async fn job(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
 ->Result<Json<Value>,Fault>{
 if let Some(job)=engine.developer.job(id).await{
  return Ok(Json(json!(job)));
 }
 // The original RSI screen also polls this path after a source adoption.
 // Resolve both job namespaces through this one route, without Axum overlap.
 if let Some(c)=engine.campaigns.get(id).await{
  let applied=c.head_commit.is_some();
  return Ok(Json(json!({"id":id,"status":if applied{"completed"}else{"not_applied"},
   "result":{"sourceApplied":applied,"requiresRebuild":applied},
   "error":Value::Null})));
 }
 Err((StatusCode::NOT_FOUND,Json(json!({"error":"developer job not found"}))))
}
async fn start(State(engine):State<Arc<Engine>>,Path(kind):Path<String>,Json(input):Json<Value>)
 ->Result<Json<Value>,Fault>{
 let started=engine.developer.start_job(kind,input).await.map_err(invalid)?;
 Ok(Json(json!(started)))
}
async fn preview_start(State(engine):State<Arc<Engine>>)->Result<Json<Value>,Fault>{
 engine.developer.start_preview().await.map(Json).map_err(invalid)
}
async fn preview_stop(State(engine):State<Arc<Engine>>)->Result<Json<Value>,Fault>{
 engine.developer.stop_preview().await.map(Json).map_err(invalid)
}
async fn update_status(State(engine):State<Arc<Engine>>)->Json<Value>{
 // Installing/updating the running binary requires a verified installer,
 // preserved user data and a restart supervisor. Never invent update status.
 let actual=engine.developer.status().await;
 Json(json!({
   "supported":false,"available":false,"local":actual["commit"],
   "running":option_env!("DOMINION_BUILD_COMMIT"),
   "reason":"No verified self-update/restart supervisor is configured for this Rust server."
 }))
}
async fn update_unavailable()->Result<Json<Value>,Fault>{
 Err(unavailable("Self-update requires a verified installer and restart supervisor; execution is not configured."))
}
async fn pick_folder(State(engine):State<Arc<Engine>>)->Json<Value>{
 // Backend cannot make a remote browser or headless server show a native
 // folder picker. Return the user-connected workspace instead of a made-up path.
 Json(json!({"path":engine.developer.get_config().await.source_dir}))
}

#[cfg(test)]
mod tests {
 use super::*;
 use crate::{config::Settings,sdk::Dominion};

 #[tokio::test]
 async fn developer_job_lookup_is_json_route_not_frontend_fallback(){
  let root=std::env::temp_dir().join(format!("dominion-dev-job-{}",Uuid::new_v4()));
  let mut settings=Settings::from_env().unwrap();
  settings.data_dir=root.join("state");
  settings.workspace_dir=root.join("workspace");
  settings.token=None;
  let sdk=Dominion::open(settings).await.unwrap();
  let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let addr=listener.local_addr().unwrap();
  let serving=tokio::spawn(async move{axum::serve(listener,sdk.http_router()).await.unwrap();});
  let url=format!("http://{addr}/api/dev/jobs/{}",Uuid::new_v4());
  let response=reqwest::get(url).await.unwrap();
  assert_eq!(response.status(),StatusCode::NOT_FOUND);
  let body=response.json::<Value>().await.unwrap();
  assert_eq!(body["error"],"developer job not found");
  serving.abort();
  let _=tokio::fs::remove_dir_all(root).await;
 }
}
