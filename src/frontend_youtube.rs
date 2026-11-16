//! Actual YouTube resumable upload jobs for native FFmpeg-rendered video files.
//! Jobs are durable and explicitly user-initiated. Uncertain uploads after a
//! crash are NOT replayed, because a replay may publish the same video twice.
use crate::{domain::now_ms,engine::Engine};
use axum::{extract::{Path,State},http::{header,StatusCode},routing::{get,post},Json,Router};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::{collections::HashMap,fs,io::Write,path::{Path as FilePath,PathBuf},
 sync::{Arc,Mutex,OnceLock},time::Duration};
use tokio_util::io::ReaderStream;
use uuid::Uuid;
type Fault=(StatusCode,Json<Value>);
const LIMIT:u64=2*1024*1024*1024;
static LOCK:OnceLock<Mutex<()>>=OnceLock::new();
fn guard()->std::sync::MutexGuard<'static,()>{
 LOCK.get_or_init(||Mutex::new(())).lock().unwrap_or_else(|e|e.into_inner())
}
fn fail(code:StatusCode,msg:impl Into<String>)->Fault{
 (code,Json(json!({"error":msg.into()})))
}
fn file(engine:&Engine)->PathBuf{engine.settings.data_dir.join("youtube-uploads.json")}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
struct Upload{
 id:Uuid,video_id:Uuid,status:String,title:String,privacy_status:String,
 #[serde(default)]progress:u8,
 #[serde(default)]result:Option<Value>,
 #[serde(default)]error:Option<String>,
 created_at_ms:u64,
}
#[derive(Default,Serialize,Deserialize)]
struct StateFile{jobs:HashMap<Uuid,Upload>}
fn read(path:&FilePath)->Result<StateFile,String>{
 if !path.exists(){return Ok(StateFile::default());}
 #[cfg(unix)]{
  use std::os::unix::fs::PermissionsExt;
  if fs::metadata(path).map_err(|e|e.to_string())?.permissions().mode()&0o077!=0{
   return Err("YouTube uploads record must be owner-only".into());
  }
 }
 serde_json::from_slice(&fs::read(path).map_err(|e|e.to_string())?)
  .map_err(|e|format!("invalid upload journal: {e}"))
}
fn write(path:&FilePath,state:&StateFile)->Result<(),String>{
 let bytes=serde_json::to_vec(state).map_err(|e|e.to_string())?;
 if bytes.len()>2_000_000{return Err("YouTube upload journal exceeds 2 MiB".into());}
 let dir=path.parent().ok_or("missing YouTube journal directory")?;
 fs::create_dir_all(dir).map_err(|e|e.to_string())?;
 let temp=dir.join(format!(".youtube-{}.tmp",Uuid::new_v4()));
 let result=(||->Result<(),String>{
  let mut opts=fs::OpenOptions::new();opts.create_new(true).write(true);
  #[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;opts.mode(0o600);}
  let mut out=opts.open(&temp).map_err(|e|e.to_string())?;
  out.write_all(&bytes).map_err(|e|e.to_string())?;
  out.sync_all().map_err(|e|e.to_string())?;
  drop(out);fs::rename(&temp,path).map_err(|e|e.to_string())?;
  #[cfg(unix)]fs::File::open(dir).and_then(|f|f.sync_all()).map_err(|e|e.to_string())?;
  Ok(())
 })();
 if result.is_err(){let _=fs::remove_file(temp);}
 result
}
fn change(engine:&Engine,id:Uuid,edit:impl FnOnce(&mut Upload))->Result<(),String>{
 let _lock=guard();
 let path=file(engine);let mut state=read(&path)?;
 let job=state.jobs.get_mut(&id).ok_or("YouTube job missing")?;
 edit(job);
 write(&path,&state)
}
fn public(job:&Upload)->Value{
 json!({"id":job.id,"videoId":job.video_id,"kind":"youtube_upload",
  "status":job.status,"title":job.title,"privacyStatus":job.privacy_status,
  "progress":job.progress,"result":job.result,"error":job.error,
  "createdAt":crate::frontend_state::timestamp(job.created_at_ms)})
}
pub fn routes()->Router<Arc<Engine>>{
 Router::new().route("/youtube/status",get(status))
  .route("/youtube/uploads",get(list))
  .route("/youtube/uploads/{id}",get(get_upload))
  .route("/video/jobs/{id}/upload",post(start_upload))
}
async fn status(State(engine):State<Arc<Engine>>)->Result<Json<Value>,Fault>{
 let connected=crate::frontend_oauth::youtube_connection(&engine.settings);
 let snapshot=read(&file(&engine)).map_err(|e|fail(StatusCode::INTERNAL_SERVER_ERROR,e))?;
 let mut uploads=snapshot.jobs.values().map(public).collect::<Vec<_>>();
 uploads.sort_by(|a,b|b["createdAt"].as_str().cmp(&a["createdAt"].as_str()));
 Ok(Json(json!({"connected":connected,"uploadAllowed":connected,
  "account":crate::frontend_oauth::youtube_public(&engine.settings),
  "uploads":uploads})))
}
async fn list(State(engine):State<Arc<Engine>>)->Result<Json<Value>,Fault>{
 let snapshot=read(&file(&engine)).map_err(|e|fail(StatusCode::INTERNAL_SERVER_ERROR,e))?;
 Ok(Json(json!(snapshot.jobs.values().map(public).collect::<Vec<_>>())))
}
async fn get_upload(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
 ->Result<Json<Value>,Fault>{
 let snapshot=read(&file(&engine)).map_err(|e|fail(StatusCode::INTERNAL_SERVER_ERROR,e))?;
 snapshot.jobs.get(&id).map(|j|Json(public(j)))
  .ok_or_else(||fail(StatusCode::NOT_FOUND,"upload not found"))
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct UploadRequest{
 title:String,
 #[serde(default)]description:String,
 #[serde(default="private")]privacy_status:String,
 #[serde(default)]tags:String,
}
fn private()->String{"private".into()}
async fn start_upload(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>,
 Json(input):Json<UploadRequest>)->Result<(StatusCode,Json<Value>),Fault>{
 if !crate::frontend_oauth::youtube_connection(&engine.settings){
  return Err(fail(StatusCode::PRECONDITION_REQUIRED,"Connect YouTube OAuth before uploading"));
 }
 if input.title.trim().is_empty()||input.title.len()>100||
  input.description.len()>4900||
  !matches!(input.privacy_status.as_str(),"private"|"unlisted"|"public"){
  return Err(fail(StatusCode::BAD_REQUEST,"YouTube title, description or privacy invalid"));
 }
 // Both the native FFmpeg renderer and ComfyUI produce durable artifacts
 // under the same Video page, and are eligible for explicit YouTube upload.
 let (path,mime)=if let Some(video)=engine.videos.get(id).await{
  if !matches!(video.status,crate::video::VideoStatus::Completed){
   return Err(fail(StatusCode::CONFLICT,"video must finish rendering before upload"));
  }
  (engine.videos.output_path(id).await.map_err(|e|fail(StatusCode::NOT_FOUND,e))?,
   "video/mp4")
 }else{
  crate::frontend_comfy::output(&engine.settings,id)
   .map_err(|e|fail(StatusCode::CONFLICT,e))?
   .ok_or_else(||fail(StatusCode::NOT_FOUND,"video artifact missing"))?
 };
 let len=tokio::fs::metadata(&path).await.map_err(|e|fail(StatusCode::NOT_FOUND,e.to_string()))?.len();
 if len==0||len>LIMIT{return Err(fail(StatusCode::BAD_REQUEST,"video must be 1 byte to 2 GiB"));}
 let tags=input.tags.split(',').map(str::trim).filter(|s|!s.is_empty())
  .take(20).map(|s|s.chars().take(70).collect::<String>()).collect::<Vec<_>>();
 let job=Upload{id:Uuid::new_v4(),video_id:id,status:"running".into(),
  title:input.title.trim().into(),privacy_status:input.privacy_status.clone(),
  progress:0,result:None,error:None,created_at_ms:now_ms()};
 {
  let _lock=guard();let root=file(&engine);
  let mut state=read(&root).map_err(|e|fail(StatusCode::INTERNAL_SERVER_ERROR,e))?;
  if state.jobs.len()>=512{return Err(fail(StatusCode::CONFLICT,"upload history limit reached"));}
  if state.jobs.values().any(|j|j.video_id==id&&j.status=="running"){
   return Err(fail(StatusCode::CONFLICT,"upload already running for this native video"));
  }
  state.jobs.insert(job.id,job.clone());
  write(&root,&state).map_err(|e|fail(StatusCode::INTERNAL_SERVER_ERROR,e))?;
 }
 let worker=Arc::clone(&engine);
 let job_id=job.id;
 tokio::spawn(async move{
  let outcome=send(&worker,&path,len,mime,&input,tags).await;
  if let Err(error)=change(&worker,job_id,|job|{
   match outcome{
    Ok(ref remote_id)=>{
     job.status="completed".into();job.progress=100;
     job.result=Some(json!({"id":remote_id,
      "url":format!("https://www.youtube.com/watch?v={remote_id}")}));
    },
    Err(ref failure)=>{job.status="failed".into();job.error=Some(failure.clone());}
   }
  }){tracing::error!(%error,job=%job_id,"YouTube upload outcome was not persisted");}
 });
 Ok((StatusCode::ACCEPTED,Json(public(&job))))
}
async fn send(engine:&Engine,path:&FilePath,len:u64,mime:&str,request:&UploadRequest,tags:Vec<String>)
 ->Result<String,String>{
 let access=crate::frontend_oauth::youtube_token(&engine.settings).await?;
 let http=reqwest::Client::builder().timeout(Duration::from_secs(900))
  .redirect(reqwest::redirect::Policy::none()).build().map_err(|e|e.to_string())?;
 let body=json!({"snippet":{"title":request.title,"description":request.description,
  "tags":tags,"categoryId":"22"},
  "status":{"privacyStatus":request.privacy_status}});
 let started=http.post("https://www.googleapis.com/upload/youtube/v3/videos?uploadType=resumable&part=snippet,status")
  .bearer_auth(&access)
  .header("X-Upload-Content-Type",mime)
  .header("X-Upload-Content-Length",len.to_string())
  .json(&body).send().await.map_err(|e|format!("YouTube session request: {e}"))?;
 if !started.status().is_success(){
  return Err(format!("YouTube session rejected HTTP {}",started.status()));
 }
 let location=started.headers().get(header::LOCATION)
  .and_then(|h|h.to_str().ok()).ok_or("YouTube upload session missing Location header")?;
 let url=reqwest::Url::parse(location).map_err(|e|e.to_string())?;
 if url.scheme()!="https"||!matches!(url.host_str(),
  Some("www.googleapis.com"|"upload.youtube.com"|"youtube.googleapis.com"))
  ||!url.username().is_empty()||url.password().is_some(){
  return Err("YouTube upload session was not on an allowed HTTPS host".into());
 }
 let input=tokio::fs::File::open(path).await.map_err(|e|e.to_string())?;
 let res=http.put(url).bearer_auth(&access)
  .header(header::CONTENT_TYPE,mime)
  .header(header::CONTENT_LENGTH,len.to_string())
  .body(reqwest::Body::wrap_stream(ReaderStream::new(input)))
  .send().await.map_err(|e|format!("YouTube upload transport: {e}"))?;
 if !res.status().is_success(){return Err(format!("YouTube upload HTTP {}",res.status()));}
 let result:Value=res.json().await.map_err(|e|e.to_string())?;
 let id=result["id"].as_str().ok_or("YouTube returned no published video ID")?;
 if id.len()<6||id.len()>64||!id.chars().all(|c|c.is_ascii_alphanumeric()||c=='-'||c=='_'){
  return Err("YouTube returned malformed video ID".into());
 }
 Ok(id.to_owned())
}
/// The autonomous producer queues through the SAME user-account upload
/// implementation as the manual Video UI. Never duplicate network logic.
pub async fn queue_upload(engine:Arc<Engine>,video_id:Uuid,title:&str,
 description:&str,privacy:&str,tags:&str)->Result<Value,String>{
 let input=UploadRequest{
  title:title.to_owned(),description:description.to_owned(),
  privacy_status:privacy.to_owned(),tags:tags.to_owned()
 };
 let (_,Json(job))=start_upload(State(engine),Path(video_id),Json(input)).await
  .map_err(|(_,Json(body))|body["error"].as_str().unwrap_or("YouTube upload rejected").to_owned())?;
 Ok(job)
}
/// Read durable upload outcomes without changing or replaying a remote upload.
pub fn lookup_upload(settings:&crate::config::Settings,id:Uuid)->Result<Option<Value>,String>{
 Ok(read(&settings.data_dir.join("youtube-uploads.json"))?.jobs.get(&id).map(public))
}

/// Uploads that were in-flight before restart are uncertain and must not be
/// automatically replayed, because the remote video might already exist.
pub fn reconcile_restart(settings:&crate::config::Settings)->Result<(),String>{
 let _lock=guard();
 let path=settings.data_dir.join("youtube-uploads.json");
 let mut data=read(&path)?;let mut changed=false;
 for job in data.jobs.values_mut(){
  if job.status=="running"{
   job.status="needs_review".into();
   job.error=Some("Server restarted during YouTube upload; inspect YouTube Studio before resubmitting".into());
   changed=true;
  }
 }
 if changed{write(&path,&data)?;}
 Ok(())
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test]
 fn upload_journal_recovery_never_replays_uncertain_publication(){
  let root=std::env::temp_dir().join(format!("dominion-youtube-{}",Uuid::new_v4()));
  let mut settings=crate::config::Settings::from_env().unwrap();
  settings.data_dir=root;
  let upload=Upload{id:Uuid::new_v4(),video_id:Uuid::new_v4(),
   status:"running".into(),title:"test".into(),privacy_status:"private".into(),
   progress:0,result:None,error:None,created_at_ms:now_ms()};
  let path=settings.data_dir.join("youtube-uploads.json");
  write(&path,&StateFile{jobs:HashMap::from([(upload.id,upload.clone())])}).unwrap();
  reconcile_restart(&settings).unwrap();
  let recorded=read(&path).unwrap();
  assert_eq!(recorded.jobs[&upload.id].status,"needs_review");
  fs::remove_dir_all(settings.data_dir).unwrap();
 }
}
