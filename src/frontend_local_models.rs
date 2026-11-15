//! Original Chat model-picker -> verified local Ollama control plane.
//! Only loopback Ollama endpoints are allowed. The selected model is routed
//! through the native Rust ModelClient; UI selection cannot silently fall back.
use crate::{config::{ModelSettings,Settings},domain::RunStatus,engine::Engine};
use axum::{extract::State,http::StatusCode,routing::{get,post},Json,Router};
use serde::Deserialize;
use serde_json::{json,Value};
use std::{fs,io::Write,path::PathBuf,sync::Arc,time::Duration};
use uuid::Uuid;
type Fault=(StatusCode,Json<Value>);
fn bad(code:StatusCode,error:impl Into<String>)->Fault{
 (code,Json(json!({"error":error.into()})))
}
fn endpoint()->Result<String,String>{
 let raw=std::env::var("DOMINION_OLLAMA_URL").unwrap_or_else(|_|"http://127.0.0.1:11434".into());
 let url=reqwest::Url::parse(&raw).map_err(|e|e.to_string())?;
 let loopback=url.host_str().is_some_and(|host|host=="localhost"||
  host.parse::<std::net::IpAddr>().is_ok_and(|ip|ip.is_loopback()));
 if !loopback || url.scheme()!="http" || url.password().is_some()
  || !url.username().is_empty()||url.query().is_some()||url.fragment().is_some(){
  return Err("Ollama runtime is restricted to loopback HTTP without URL credentials".into());
 }
 Ok(raw.trim_end_matches('/').to_owned())
}
fn selection_file(settings:&Settings)->PathBuf{
 settings.data_dir.join("local-ollama-model.json")
}
pub fn selected(settings:&Settings)->Option<String>{
 let bytes=fs::read(selection_file(settings)).ok()?;
 let value:Value=serde_json::from_slice(&bytes).ok()?;
 let id=value["model"].as_str()?;
 if valid_model(id){Some(id.to_owned())}else{None}
}
fn valid_model(id:&str)->bool{
 !id.trim().is_empty()&&id.len()<=240&&
 !id.chars().any(char::is_control)
}
pub fn native_override(settings:&Settings,model:&str)->Option<ModelSettings>{
 if selected(settings).as_deref()!=Some(model){return None;}
 let url=endpoint().ok()?;
 let mut configured=settings.main.clone();
 configured.name=model.to_owned();
 configured.base_url=format!("{url}/v1");
 configured.api_key.clear();
 configured.input_krw_per_million=0.0;
 configured.cached_input_krw_per_million=0.0;
 configured.output_krw_per_million=0.0;
 configured.cache_write_krw_per_million=0.0;
 configured.cache_ttl=None;configured.cache_key=None;
 Some(configured)
}
fn save_selection(settings:&Settings,model:Option<&str>)->Result<(),String>{
 let file=selection_file(settings);
 if let Some(model)=model{
  fs::create_dir_all(&settings.data_dir).map_err(|e|e.to_string())?;
  let stage=settings.data_dir.join(format!(".ollama-{}.tmp",Uuid::new_v4()));
  let result=(||->Result<(),String>{
   let mut opt=fs::OpenOptions::new();opt.create_new(true).write(true);
   #[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;opt.mode(0o600);}
   let mut out=opt.open(&stage).map_err(|e|e.to_string())?;
   out.write_all(json!({"model":model}).to_string().as_bytes()).map_err(|e|e.to_string())?;
   out.sync_all().map_err(|e|e.to_string())?;
   drop(out);
   fs::rename(&stage,&file).map_err(|e|e.to_string())?;
   #[cfg(unix)]fs::File::open(&settings.data_dir)
    .and_then(|f|f.sync_all()).map_err(|e|e.to_string())?;
   Ok(())
  })();
  if result.is_err(){let _=fs::remove_file(stage);}
  result
 }else{
  if file.exists(){fs::remove_file(file).map_err(|e|e.to_string())?;}
  Ok(())
 }
}
async fn client()->Result<(reqwest::Client,String),String>{
 let host=endpoint()?;
 let http=reqwest::Client::builder().timeout(Duration::from_secs(90))
  .redirect(reqwest::redirect::Policy::none()).build().map_err(|e|e.to_string())?;
 Ok((http,host))
}
async fn ollama_get(route:&str)->Result<Value,String>{
 let (http,host)=client().await?;
 let result=http.get(format!("{host}{route}")).timeout(Duration::from_secs(8))
  .send().await.map_err(|e|format!("Ollama unavailable: {e}"))?;
 let status=result.status();
 if !status.is_success(){return Err(format!("Ollama HTTP {status}"));}
 result.json::<Value>().await.map_err(|e|e.to_string())
}
async fn ollama_post(body:Value)->Result<(),String>{
 let (http,host)=client().await?;
 let result=http.post(format!("{host}/api/generate"))
  .json(&body).send().await.map_err(|e|format!("Ollama load request failed: {e}"))?;
 let status=result.status();
 if !status.is_success(){
  let text=result.text().await.unwrap_or_default();
  return Err(format!("Ollama HTTP {status}: {}",text.chars().take(400).collect::<String>()));
 }
 Ok(())
}
fn names(json:&Value)->Vec<String>{
 json["models"].as_array().map(|rows|rows.iter()
  .filter_map(|row|row["name"].as_str().or_else(||row["model"].as_str()))
  .filter(|name|valid_model(name)).map(str::to_owned).collect()).unwrap_or_default()
}
pub fn routes()->Router<Arc<Engine>>{
 Router::new()
  .route("/model-library/installed",get(installed))
  .route("/model-library/runtime",get(runtime))
  .route("/model-library/runtime/load",post(load))
  .route("/model-library/runtime/unload",post(unload))
}
async fn installed()->Result<Json<Value>,Fault>{
 let result=match ollama_get("/api/tags").await{
  Ok(data)=>data,
  Err(_)=>return Ok(Json(json!([]))), // No Ollama process or models are installed.
 };
 let list=result["models"].as_array().map(|rows|rows.iter().filter_map(|row|{
  let id=row["name"].as_str().or_else(||row["model"].as_str())?;
  if !valid_model(id){return None;}
  Some(json!({"id":id,"repo":id,"label":id,"format":row["details"]["format"].as_str().unwrap_or("ollama"),
   "bytes":row["size"].as_u64().unwrap_or(0),"source":"ollama"}))
 }).collect::<Vec<_>>()).unwrap_or_default();
 Ok(Json(json!(list)))
}
async fn runtime(State(engine):State<Arc<Engine>>)->Json<Value>{
 let Some(model)=selected(&engine.settings) else{
  return Json(json!({"status":"idle","engine":"ollama","model":Value::Null}));
 };
 match ollama_get("/api/ps").await{
  Ok(ps) if names(&ps).iter().any(|name|name==&model)=>{
   Json(json!({"status":"ready","engine":"ollama","id":model,"name":model,"model":model}))
  },
  Ok(_)=>Json(json!({"status":"idle","engine":"ollama",
   "error":"The selected model is not currently loaded in Ollama"})),
  Err(error)=>Json(json!({"status":"unavailable","engine":"ollama","error":error})),
 }
}
#[derive(Deserialize)]
struct Load{ id:String,#[serde(default)]context:Option<u32> }
async fn load(State(engine):State<Arc<Engine>>,Json(input):Json<Load>)
 ->Result<Json<Value>,Fault>{
 if !valid_model(&input.id){return Err(bad(StatusCode::BAD_REQUEST,"invalid local model ID"));}
 if input.context.is_some_and(|value|!(256..=262144).contains(&value)){
  return Err(bad(StatusCode::BAD_REQUEST,"context must be 256-262144 tokens"));
 }
 if engine.store.list().await.iter().any(|run|!run.terminal()&&
  (run.provider_id.is_empty()||run.provider_id=="dominion-native")){
  return Err(bad(StatusCode::CONFLICT,"finish active native-model runs before switching the local runtime"));
 }
 let tags=ollama_get("/api/tags").await.map_err(|e|bad(StatusCode::SERVICE_UNAVAILABLE,e))?;
 if !names(&tags).iter().any(|name|name==&input.id){
  return Err(bad(StatusCode::NOT_FOUND,"requested Ollama model is not installed"));
 }
 let mut body=json!({"model":input.id,"prompt":"","stream":false,"keep_alive":"24h"});
 if let Some(context)=input.context{body["options"]=json!({"num_ctx":context});}
 ollama_post(body).await.map_err(|e|bad(StatusCode::BAD_GATEWAY,e))?;
 let ps=ollama_get("/api/ps").await.map_err(|e|bad(StatusCode::BAD_GATEWAY,e))?;
 if !names(&ps).iter().any(|name|name==&input.id){
  return Err(bad(StatusCode::BAD_GATEWAY,"Ollama did not report the requested model as loaded"));
 }
 save_selection(&engine.settings,Some(&input.id)).map_err(|e|bad(StatusCode::INTERNAL_SERVER_ERROR,e))?;
 Ok(Json(json!({"status":"ready","engine":"ollama","id":input.id,
  "name":input.id,"model":input.id})))
}
async fn unload(State(engine):State<Arc<Engine>>)->Result<Json<Value>,Fault>{
 let Some(model)=selected(&engine.settings) else{
  return Ok(Json(json!({"status":"idle","engine":"ollama"})));
 };
 if engine.store.list().await.iter().any(|run|!run.terminal()&&
  (run.provider_id.is_empty()||run.provider_id=="dominion-native")){
  return Err(bad(StatusCode::CONFLICT,"native model has active runs"));
 }
 ollama_post(json!({"model":model,"prompt":"","stream":false,"keep_alive":0}))
  .await.map_err(|e|bad(StatusCode::BAD_GATEWAY,e))?;
 let ps=ollama_get("/api/ps").await.map_err(|e|bad(StatusCode::BAD_GATEWAY,e))?;
 if names(&ps).iter().any(|name|name==&model){
  return Err(bad(StatusCode::BAD_GATEWAY,"model remains resident after unload"));
 }
 save_selection(&engine.settings,None).map_err(|e|bad(StatusCode::INTERNAL_SERVER_ERROR,e))?;
 Ok(Json(json!({"status":"idle","engine":"ollama"})))
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test]
 fn local_model_identifiers_and_ollama_receipts_are_bounded(){
  assert!(valid_model("qwen3:8b"));
  assert!(!valid_model("\r\n"));
  assert_eq!(names(&json!({"models":[{"name":"qwen3:8b"}]})),vec!["qwen3:8b"]);
  assert!(names(&json!({"models":[]})).is_empty());
 }
}
