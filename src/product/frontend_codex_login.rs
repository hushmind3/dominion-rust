//! Local, explicit Codex CLI device authentication adapter. No OAuth token
//! enters the Rust model registry; login alone never creates a chat provider.
use crate::{domain::now_ms,engine::Engine};
use axum::{extract::State,http::StatusCode,routing::{get,post},Json,Router};
use serde_json::{json,Value};
use std::{process::Stdio,sync::{Arc,OnceLock},time::Duration};
use tokio::{io::{AsyncRead,AsyncReadExt},process::Command,sync::{Mutex,oneshot}};
type Fault=(StatusCode,Json<Value>);
fn fail(code:StatusCode,msg:impl Into<String>)->Fault{
 (code,Json(json!({"error":msg.into()})))
}
const URL:&str="https://auth.openai.com/codex/device";
struct Progress{phase:String,code:Option<String>,message:String,started:u64}
struct Login{progress:Mutex<Progress>,cancel:Mutex<Option<oneshot::Sender<()>>>}
static CURRENT:OnceLock<Mutex<Option<Arc<Login>>>>=OnceLock::new();
fn current()->&'static Mutex<Option<Arc<Login>>>{
 CURRENT.get_or_init(||Mutex::new(None))
}
async fn command_ok(args:&[&str],duration:u64)->bool{
 tokio::time::timeout(Duration::from_secs(duration),
  Command::new("codex").args(args).kill_on_drop(true).output())
  .await.ok().and_then(Result::ok).is_some_and(|out|out.status.success())
}
pub(crate) async fn installed()->bool{command_ok(&["--version"],4).await}
pub(crate) async fn authorized()->bool{command_ok(&["login","status"],5).await}
fn code(text:&str)->Option<String>{
 for word in text.split_whitespace(){
  let word=word.trim_matches(|c:char|!c.is_ascii_alphanumeric()&&c!='-');
  if let Some((a,b))=word.split_once('-'){
   if (4..=5).contains(&a.len())&&(4..=5).contains(&b.len())
    &&a.bytes().chain(b.bytes()).all(|c|c.is_ascii_uppercase()||c.is_ascii_digit()){
    return Some(word.to_owned())
   }
  }
 }
 None
}
async fn read<R:AsyncRead+Unpin>(mut source:R,shared:Arc<Login>){
 let mut buf=String::new();let mut chunk=[0u8;1024];
 loop{
  let n=match source.read(&mut chunk).await{Ok(0)|Err(_)=>break,Ok(n)=>n};
  buf.push_str(&String::from_utf8_lossy(&chunk[..n]));
  if buf.len()>3072{
   let pos=buf.char_indices().find(|(i,_)|*i>=buf.len()-1536)
    .map_or(0,|(i,_)|i);
   buf.drain(..pos);
  }
  if let Some(found)=code(&buf){
   let mut p=shared.progress.lock().await;
   if p.phase=="starting"||p.phase=="device_pending"{
    p.phase="device_pending".into();p.code=Some(found);
    p.message="공식 Codex 기기 인증 화면에서 코드를 입력하세요.".into();
   }
  }
 }
}
fn public(p:&Progress)->Value{
 json!({"status":p.phase,"userCode":p.code,"verificationUrl":URL,
  "startedAt":p.started,"message":p.message,"nativeProviderConnected":false})
}
async fn status()->Json<Value>{
 let shared=current().lock().await.clone();
 if let Some(login)=shared{return Json(public(&*login.progress.lock().await))}
 if authorized().await{
  Json(json!({"status":"success","message":"Codex CLI 로그인 완료. Rust 모델 API는 별도로 연결해야 합니다.","nativeProviderConnected":false}))
 }else{Json(json!({"status":"idle","nativeProviderConnected":false}))}
}
async fn start(State(engine):State<Arc<Engine>>)->Result<Json<Value>,Fault>{
 if !crate::frontend_oauth::local_cli_eligible(&engine.settings){
  return Err(fail(StatusCode::FORBIDDEN,"CLI login requires a loopback server"));
 }
 if !installed().await{return Err(fail(StatusCode::FAILED_DEPENDENCY,"Codex CLI not installed"))}
 if authorized().await{
  return Ok(Json(json!({"user_code":Value::Null,"verification_url":URL,
   "status":"success","message":"Codex CLI already authenticated"})));
 }
 let mut slot=current().lock().await;
 let session=match slot.as_ref(){
  Some(existing) if matches!(existing.progress.lock().await.phase.as_str(),"starting"|"device_pending")=>Arc::clone(existing),
  _=>{
   let mut child=Command::new("codex").args(["login","--device-auth"])
    .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
    .kill_on_drop(true).env("NO_COLOR","1").spawn()
    .map_err(|e|fail(StatusCode::BAD_GATEWAY,format!("Codex login failed to start: {e}")))?;
   let (tx,mut rx)=oneshot::channel();
   let shared=Arc::new(Login{progress:Mutex::new(Progress{
    phase:"starting".into(),code:None,started:now_ms(),
    message:"Codex 기기 인증을 준비하고 있습니다.".into()}),
    cancel:Mutex::new(Some(tx))});
   if let Some(stdout)=child.stdout.take(){tokio::spawn(read(stdout,Arc::clone(&shared)));}
   if let Some(stderr)=child.stderr.take(){tokio::spawn(read(stderr,Arc::clone(&shared)));}
   let worker=Arc::clone(&shared);
   tokio::spawn(async move{
    let (state,message)=tokio::select!{
     done=child.wait()=>match done{
      Ok(status) if status.success()&&authorized().await=>("success","Codex CLI 인증 완료. Rust 모델 API는 별도로 연결하세요."),
      _=>("error","Codex CLI 인증이 완료되지 않았습니다.")},
     _=&mut rx=>{let _=child.kill().await;("cancelled","Codex 인증 취소됨")},
     _=tokio::time::sleep(Duration::from_secs(600))=>{
      let _=child.kill().await;("error","Codex 인증 시간이 초과됐습니다.")}
    };
    let mut p=worker.progress.lock().await;
    p.phase=state.into();p.message=message.into();
    *worker.cancel.lock().await=None;
   });
   *slot=Some(Arc::clone(&shared));shared
  }
 };
 drop(slot);
 for _ in 0..100{
  let p=session.progress.lock().await;
  if p.phase=="device_pending"{
   return Ok(Json(json!({"user_code":p.code,"verification_url":URL,"status":p.phase})))
  }
  if p.phase=="error"||p.phase=="cancelled"{
   return Err(fail(StatusCode::BAD_GATEWAY,p.message.clone()))
  }
  drop(p);tokio::time::sleep(Duration::from_millis(150)).await;
 }
 Err(fail(StatusCode::GATEWAY_TIMEOUT,"Codex device code is still being prepared; retry login"))
}
async fn browser(State(engine):State<Arc<Engine>>)->Result<Json<Value>,Fault>{
 let Json(value)=start(State(engine)).await?;
 Ok(Json(json!({"url":URL,"userCode":value["user_code"],"status":value["status"]})))
}
async fn cancel(State(engine):State<Arc<Engine>>)->Result<Json<Value>,Fault>{
 if !crate::frontend_oauth::local_cli_eligible(&engine.settings){
  return Err(fail(StatusCode::FORBIDDEN,"CLI login is local only"));
 }
 let login=current().lock().await.clone();
 if let Some(login)=login{
  if let Some(tx)=login.cancel.lock().await.take(){let _=tx.send(());}
 }
 Ok(Json(json!({"ok":true})))
}
pub fn routes()->Router<Arc<Engine>>{
 Router::new()
 .route("/providers/codex/device",post(start))
 .route("/providers/codex/login",get(browser))
 .route("/providers/codex/login/status",get(status))
 .route("/providers/codex/login/cancel",post(cancel))
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test]fn code_and_response_do_not_expose_auth_tokens(){
  assert_eq!(code("Enter code: ABCD-EFGH"),Some("ABCD-EFGH".into()));
  assert_eq!(code("Bearer sk-ABCDEFGHIJK"),None);
  assert_eq!(code("https://auth.openai.com/codex/device"),None);
  let p=Progress{phase:"device_pending".into(),code:Some("ABCD-EFGH".into()),
   message:"ready".into(),started:123};
  assert_eq!(public(&p)["nativeProviderConnected"],false);
  assert!(!public(&p).to_string().contains("access_token"));
 }
}
