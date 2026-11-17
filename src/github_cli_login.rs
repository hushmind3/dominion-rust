//! Local GitHub device login using the official GitHub CLI.
//! Never accepts commands or tokens from browser input, and is loopback-only.
use crate::{domain::now_ms,engine::Engine};
use axum::{extract::State,http::StatusCode,routing::{get,post},Json,Router};
use serde_json::{json,Value};
use std::{process::Stdio,sync::{Arc,OnceLock},time::Duration};
use tokio::{io::{AsyncRead,AsyncReadExt,AsyncWriteExt},process::Command,
 sync::{Mutex,oneshot}};

type Fault=(StatusCode,Json<Value>);
fn bad(error:impl Into<String>)->Fault{
 (StatusCode::BAD_REQUEST,Json(json!({"error":error.into()})))
}
struct Progress{phase:&'static str,code:Option<String>,message:String,started_ms:u64}
struct Login{progress:Mutex<Progress>,cancel:Mutex<Option<oneshot::Sender<()>>>}
static ACTIVE:OnceLock<Mutex<Option<Arc<Login>>>>=OnceLock::new();
fn active()->&'static Mutex<Option<Arc<Login>>>{
 ACTIVE.get_or_init(||Mutex::new(None))
}
fn code(input:&str)->Option<String>{
 let text=input.as_bytes();
 for (offset,chunk) in text.windows(9).enumerate(){
  if chunk[4]!=b'-'||!chunk.iter().enumerate().all(|(i,b)|
    i==4||b.is_ascii_uppercase()||b.is_ascii_digit()){continue;}
  if offset>0&&text[offset-1].is_ascii_alphanumeric(){continue;}
  if text.get(offset+9).is_some_and(u8::is_ascii_alphanumeric){continue;}
  return String::from_utf8(chunk.to_vec()).ok();
 }
 None
}
fn command()->Command{
 #[cfg(target_os="macos")]
 let mut child={
  let mut c=Command::new("/usr/bin/script");
  c.args(["-q","/dev/null","gh","auth","login",
   "--hostname","github.com","--git-protocol","https","--web",
   "--skip-ssh-key","--scopes","repo,read:user,user:email"]);c
 };
 #[cfg(all(unix,not(target_os="macos")))]
 let mut child={
  // PTY is needed for the GitHub CLI "Press Enter" browser login flow.
  // The process string is constant; no request/model input is interpolated.
  let mut c=Command::new("script");
  c.args(["-q","-f","-c",
   "gh auth login --hostname github.com --git-protocol https --web --skip-ssh-key --scopes repo,read:user,user:email",
   "/dev/null"]);c
 };
 #[cfg(windows)]
 let mut child={
  let mut c=Command::new("gh");
  c.args(["auth","login","--hostname","github.com","--git-protocol",
   "https","--web","--skip-ssh-key","--scopes","repo,read:user,user:email"]);c
 };
 child.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
  .kill_on_drop(true).env("NO_COLOR","1").env("TERM","xterm-256color");
 child
}
async fn phase(login:&Login,status:&'static str,message:&str){
 let mut p=login.progress.lock().await;
 if !matches!(p.phase,"completed"|"failed"|"cancelled"){
  p.phase=status;p.message=message.to_owned();
 }
}
async fn gh_authorized()->bool{
 tokio::time::timeout(Duration::from_secs(5),Command::new("gh")
  .args(["auth","status","--hostname","github.com"])
  .kill_on_drop(true).output()).await
  .is_ok_and(|v|v.is_ok_and(|output|output.status.success()))
}
async fn capture<R:AsyncRead+Unpin>(mut stream:R,login:Arc<Login>){
 let mut buffer=String::new();
 let mut bytes=[0;2048];
 loop{
  let n=match stream.read(&mut bytes).await{Ok(0)|Err(_)=>break,Ok(n)=>n};
  buffer.push_str(&String::from_utf8_lossy(&bytes[..n]));
  if buffer.len()>4096{
   let index=buffer.char_indices()
    .find(|(i,_)|*i>=buffer.len()-2048).map_or(0,|(i,_)|i);
   buffer.drain(..index);
  }
  if let Some(found)=code(&buffer){
   let mut progress=login.progress.lock().await;
   if !matches!(progress.phase,"completed"|"failed"|"cancelled"){
    progress.code=Some(found);progress.phase="waiting";
    progress.message="GitHub 인증 페이지에서 표시된 1회용 코드를 입력하세요.".into();
   }
  }
 }
}
fn public(progress:&Progress)->Value{
 json!({"phase":progress.phase,"message":progress.message,
  "verificationUrl":"https://github.com/login/device","userCode":progress.code,
  "startedAt":progress.started_ms})
}
async fn status()->Json<Value>{
 let current=active().lock().await.clone();
 match current{Some(login)=>Json(public(&*login.progress.lock().await)),
  None=>Json(json!({"phase":"idle"}))}
}
async fn start(State(engine):State<Arc<Engine>>)->Result<Json<Value>,Fault>{
 if !crate::frontend_oauth::local_cli_eligible(&engine.settings){
  return Err(bad("Local GitHub device login requires a loopback-only Dominion instance"));
 }
 if !crate::frontend_oauth::github_cli_installed().await{
  return Err(bad("GitHub CLI is missing; install gh first"));
 }
 let mut slot=active().lock().await;
 if let Some(existing)=slot.as_ref(){
  let progress=existing.progress.lock().await;
  if matches!(progress.phase,"starting"|"waiting"){
   return Ok(Json(public(&*progress)));
  }
 }
 let mut child=command().spawn().map_err(|e|bad(format!("GitHub CLI login launch failed: {e}")))?;
 let (tx,mut rx)=oneshot::channel();
 let login=Arc::new(Login{progress:Mutex::new(Progress{
  phase:"starting",code:None,started_ms:now_ms(),
  message:"GitHub 인증 페이지를 준비하고 있습니다.".into()
 }),cancel:Mutex::new(Some(tx))});
 *slot=Some(login.clone());drop(slot);
 let worker=login.clone();
 tokio::spawn(async move{
  if let Some(reader)=child.stdout.take(){tokio::spawn(capture(reader,worker.clone()));}
  if let Some(reader)=child.stderr.take(){tokio::spawn(capture(reader,worker.clone()));}
  if let Some(mut input)=child.stdin.take(){
   tokio::spawn(async move{let _=input.write_all(b"\n").await;});
  }
  let (state,message)=tokio::select!{
   result=child.wait()=>match result{
    Ok(exit) if exit.success() && gh_authorized().await=>
      ("completed","GitHub 인증을 마쳤습니다."),
    _=>("failed","GitHub CLI 인증이 완료되지 않았습니다. 다시 로그인하세요.")
   },
   _=&mut rx=>{let _=child.kill().await;("cancelled","GitHub 인증이 취소됐습니다.")},
   _=tokio::time::sleep(Duration::from_secs(600))=>{
    let _=child.kill().await;("failed","GitHub 인증을 10분 안에 완료하지 못했습니다.")
   }
  };
  phase(&worker,state,message).await;
  *worker.cancel.lock().await=None;
 });
 Ok(Json(public(&*login.progress.lock().await)))
}
async fn cancel(State(engine):State<Arc<Engine>>)->Result<Json<Value>,Fault>{
 if !crate::frontend_oauth::local_cli_eligible(&engine.settings){
  return Err(bad("GitHub device login cancellation is local-only"));
 }
 if let Some(login)=active().lock().await.clone(){
  if let Some(tx)=login.cancel.lock().await.take(){let _=tx.send(());}
 }
 Ok(Json(json!({"ok":true})))
}
pub fn routes()->Router<Arc<Engine>>{
 Router::new()
  .route("/oauth/github/cli/login",post(start))
  .route("/oauth/github/cli/login/status",get(status))
  .route("/oauth/github/cli/login/cancel",post(cancel))
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test]
 fn device_code_is_scoped_and_no_access_token_is_exposed(){
  assert_eq!(code("First copy your code: ABCD-1234\n"),Some("ABCD-1234".into()));
  assert_eq!(code("secret ghp_ABCDEF12345"),None);
  assert_eq!(code("XXABCD-1234QQ"),None);
  assert_eq!(code("ABCD-123"),None);
  let state=Progress{phase:"waiting",code:Some("ABCD-1234".into()),
   message:"ready".into(),started_ms:123};
  let exported=public(&state).to_string();
  assert!(exported.contains("https://github.com/login/device"));
  assert!(!exported.contains("access_token")&&!exported.contains("client_secret"));
 }
}
