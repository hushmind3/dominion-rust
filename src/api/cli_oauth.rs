//! Official CLI OAuth broker: runs the login behind a pseudo terminal, not Terminal.app.
//! OAuth credentials remain with the CLI; DOMINION never imports subscription tokens.
use axum::{extract::Path,http::StatusCode,response::{IntoResponse,Response},Json};
use serde::Deserialize;
use serde_json::{json,Value};
use std::{collections::HashMap,path::{Path as FilePath,PathBuf},process::Stdio,sync::{Arc,OnceLock},time::{Duration,UNIX_EPOCH}};
use tokio::{io::{AsyncRead,AsyncReadExt,AsyncWriteExt},process::{ChildStdin,Command},sync::{Mutex,oneshot}};
use super::response::error;

#[derive(Deserialize,Default)]
pub struct StartBody { #[serde(default)] provider:Option<String> }
#[derive(Deserialize)]
pub struct InputBody { input:String }

struct Progress { phase:&'static str, message:String, url:Option<String>, text:String }
struct Login { progress:Mutex<Progress>, input:Mutex<Option<ChildStdin>>, cancel:Mutex<Option<oneshot::Sender<()>>> }
fn logins()->&'static Mutex<HashMap<String,Arc<Login>>>{
    static LOGIN:OnceLock<Mutex<HashMap<String,Arc<Login>>>>=OnceLock::new();
    LOGIN.get_or_init(||Mutex::new(HashMap::new()))
}
fn command(source:&str, provider:Option<&str>)->Result<(&'static str,Vec<String>),String>{
    match source {
        "claude"=>Ok(("claude",vec!["auth".into(),"login".into()])),
        "gemini"=>Ok(("gemini",vec![])),
        "opencode"=>{
            let id=provider.unwrap_or("openai");
            if id.is_empty()||id.len()>64||!id.bytes().all(|b|b.is_ascii_lowercase()||b.is_ascii_digit()||b==b'-'){
                return Err("OpenCode 인증 제공자 ID 형식이 올바르지 않습니다.".into());
            }
            Ok(("opencode",vec!["auth".into(),"login".into(),id.into()]))
        }
        _=>Err("지원하지 않는 인증 소스입니다.".into())
    }
}
fn home()->Option<PathBuf>{std::env::var_os("HOME").map(PathBuf::from)}
fn binary(name:&str)->Option<PathBuf>{
    let mut dirs=Vec::new();
    if let Some(h)=home(){dirs.push(h.join(".local/bin"));dirs.push(h.join(".bun/bin"));}
    dirs.extend(["/opt/homebrew/bin","/usr/local/bin","/usr/bin"].into_iter().map(PathBuf::from));
    if let Some(p)=std::env::var_os("PATH"){dirs.extend(std::env::split_paths(&p));}
    dirs.into_iter().flat_map(|dir|["",".exe",".cmd"].into_iter().map(move |ext|dir.join(format!("{name}{ext}"))))
        .find(|path|path.is_file())
}
fn credential_marker(source:&str)->Option<u128>{
    let h=home()?;
    let paths:Vec<PathBuf>=match source{
        "claude"=>vec![h.join(".claude/.credentials.json")],
        "gemini"=>vec![h.join(".gemini/oauth_creds.json")],
        "opencode"=>vec![h.join(".local/share/opencode/opencode.db"),h.join(".local/share/opencode/auth.json")],
        _=>return None
    };
    paths.into_iter().filter_map(|path|{
        let m=std::fs::metadata(path).ok()?;
        Some(m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_nanos() ^ u128::from(m.len()))
    }).max()
}
fn authorization_url(text:&str)->Option<String>{
    for (pos,_) in text.match_indices("https://"){
        let candidate:String=text[pos..].chars().take(3200)
            .take_while(|c|!c.is_whitespace()&&!matches!(c,'\u{1b}'|'<'|'>'|'"'|'\''|')'|']'|'}')).collect();
        let candidate=candidate.trim_end_matches(['.',',',';']);
        if let Ok(url)=reqwest::Url::parse(candidate){
            if url.scheme()=="https"&&url.host_str().is_some_and(|host|host!="localhost"&&host!="127.0.0.1"){
                return Some(candidate.to_string());
            }
        }
    }
    None
}
/// Check CLI-owned authentication without reading or copying OAuth secrets.
pub(super) async fn credential_verified(source:&str)->bool{
    if source=="gemini"{
        return home().and_then(|h|std::fs::read(h.join(".gemini/oauth_creds.json")).ok())
            .and_then(|raw|serde_json::from_slice::<Value>(&raw).ok())
            .is_some_and(|v|v["access_token"].as_str().is_some()||v["refresh_token"].as_str().is_some());
    }
    let name=match source{"claude"=>"claude","opencode"=>"opencode",_=>return false};
    let Some(exe)=binary(name)else{return false;};
    let args:&[&str]=if source=="claude"{&["auth","status","--json"]}else{&["auth","list","--format","json"]};
    let output=tokio::time::timeout(Duration::from_secs(5),Command::new(exe).args(args).kill_on_drop(true).output()).await;
    let Ok(Ok(output))=output else{return false;};
    if !output.status.success(){return false;}
    let Ok(value)=serde_json::from_slice::<Value>(&output.stdout)else{return false;};
    if source=="claude"{
        return value["loggedIn"].as_bool()==Some(true);
    }
    // OpenCode 2 stores auth in SQLite. Ask its CLI rather than guessing
    // from the mere existence of the database.
    if let Some(array)=value.as_array(){return !array.is_empty();}
    if let Some(array)=value["accounts"].as_array(){return !array.is_empty();}
    if let Some(array)=value["connections"].as_array(){return !array.is_empty();}
    if let Some(array)=value["providers"].as_array(){return !array.is_empty();}
    value.as_object().is_some_and(|entries|!entries.is_empty())
}

async fn state(source:&str,login:&Arc<Login>)->Value{
    let p=login.progress.lock().await;
    json!({"source":source,"status":p.phase,"message":p.message,"url":p.url,"requiresInput":p.phase=="input"})
}
async fn set_phase(login:&Arc<Login>, phase:&'static str, message:&str){
    let mut p=login.progress.lock().await;
    if !matches!(p.phase,"completed"|"cancelled"|"failed"){
        p.phase=phase;p.message=message.into();
    }
}
async fn capture<R:AsyncRead+Unpin>(mut stream:R,login:Arc<Login>){
    let mut buf=[0u8;4096];
    loop{
        let n=match stream.read(&mut buf).await{Ok(0)|Err(_)=>break,Ok(n)=>n};
        let mut p=login.progress.lock().await;
        p.text.push_str(&String::from_utf8_lossy(&buf[..n]));
        if p.text.len()>18000{
            let target=p.text.len()-9000;
            let cut=p.text.char_indices().find(|(i,_)|*i>=target).map(|(i,_)|i).unwrap_or(0);
            p.text.drain(..cut);
        }
        if matches!(p.phase,"completed"|"cancelled"|"failed"){continue;}
        if p.url.is_none(){
            if let Some(url)=authorization_url(&p.text){
                p.url=Some(url);p.phase="browser";
                p.message="공식 로그인 창에서 인증을 완료하세요.".into();
            }
        }
        let lower=p.text.to_ascii_lowercase();
        if lower.contains("paste code")||lower.contains("verification code")||lower.contains("enter code"){
            p.phase="input";p.message="브라우저 인증 코드를 입력하세요.".into();
        }else if p.url.is_none()&&(lower.contains("select")||lower.contains("choose")||lower.contains("press enter")){
            p.phase="input";p.message="CLI 인증 방식 선택이 필요합니다. 목록의 인증 방식을 선택하세요.".into();
        }
    }
}
fn make_process(exe:&FilePath,args:&[String])->Command{
    #[cfg(target_os="macos")]
    let mut p={
        let mut c=Command::new("/usr/bin/script");
        c.arg("-q").arg("/dev/null").arg(exe).args(args);c
    };
    #[cfg(all(unix,not(target_os="macos")))]
    let mut p={
        fn quote(s:&str)->String{format!("'{}'",s.replace('\'',"'\\''"))}
        let line=std::iter::once(quote(&exe.to_string_lossy()))
            .chain(args.iter().map(|a|quote(a))).collect::<Vec<_>>().join(" ");
        let mut c=Command::new("script");
        c.args(["-q","-f","-c",&line,"/dev/null"]);c
    };
    #[cfg(windows)]
    let mut p={let mut c=Command::new(exe);c.args(args);c};
    if let Some(h)=home(){p.current_dir(h);}
    p.env("TERM","xterm-256color").env("COLUMNS","240").env("NO_COLOR","1");
    p.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    p
}
pub async fn start(Path(source):Path<String>,Json(body):Json<StartBody>)->Response{
    let (bin,args)=match command(&source,body.provider.as_deref()){
        Ok(v)=>v,Err(e)=>return error(StatusCode::BAD_REQUEST,&e)
    };
    if let Some(existing)=logins().lock().await.get(&source).cloned(){
        let active=!matches!(existing.progress.lock().await.phase,"completed"|"failed"|"cancelled");
        if active{return Json(state(&source,&existing).await).into_response();}
    }
    let Some(exe)=binary(bin)else{
        return error(StatusCode::NOT_FOUND,&format!("{bin} CLI 실행 파일이 없습니다. 먼저 설치하세요."));
    };
    let before=credential_marker(&source);
    let mut child=match make_process(&exe,&args).spawn(){
        Ok(c)=>c,Err(e)=>return error(StatusCode::BAD_GATEWAY,&format!("CLI 실행 실패: {e}"))
    };
    let Some(input)=child.stdin.take()else{
        return error(StatusCode::INTERNAL_SERVER_ERROR,"인증 입력 채널 생성 실패");
    };
    let (tx,mut rx)=oneshot::channel();
    let login=Arc::new(Login{
        progress:Mutex::new(Progress{phase:"starting",message:"공식 브라우저 인증을 준비하고 있습니다.".into(),url:None,text:String::new()}),
        input:Mutex::new(Some(input)),cancel:Mutex::new(Some(tx))
    });
    logins().lock().await.insert(source.clone(),login.clone());
    if let Some(out)=child.stdout.take(){tokio::spawn(capture(out,login.clone()));}
    if let Some(err)=child.stderr.take(){tokio::spawn(capture(err,login.clone()));}
    let current=login.clone();
    let kind=source.clone();
    tokio::spawn(async move{
        let mut tick=tokio::time::interval(Duration::from_secs(2));
        let timeout=tokio::time::sleep(Duration::from_secs(600));
        tokio::pin!(timeout);
        loop{
            tokio::select!{
                exit=child.wait()=>{
                    match exit{
                        Ok(exit) if exit.success()&&credential_verified(&kind).await=>set_phase(&current,"completed","CLI 인증이 확인됐습니다. 모델 연결 상태는 별도로 확인하세요.").await,
                        Ok(exit)=>set_phase(&current,"failed",&format!("로그인 완료 정보를 찾지 못했습니다 (종료 상태: {exit}).")).await,
                        Err(e)=>set_phase(&current,"failed",&format!("CLI 인증 오류: {e}")).await
                    };break;
                },
                _=&mut rx=>{let _=child.kill().await;set_phase(&current,"cancelled","인증을 취소했습니다.").await;break;},
                _=&mut timeout=>{let _=child.kill().await;set_phase(&current,"failed","로그인 시간이 10분을 초과했습니다.").await;break;},
                _=tick.tick()=>{
                    let after=credential_marker(&kind);
                    if after.is_some()&&after!=before&&credential_verified(&kind).await{
                        let _=child.kill().await;
                        set_phase(&current,"completed","CLI 인증이 확인됐습니다.").await;
                        break;
                    }
                }
            }
        }
        *current.input.lock().await=None;
    });
    Json(state(&source,&login).await).into_response()
}
pub async fn status(Path(source):Path<String>)->Response{
    match logins().lock().await.get(&source).cloned(){
        Some(login)=>Json(state(&source,&login).await).into_response(),
        None=>Json(json!({"source":source,"status":"idle","message":"로그인을 시작하세요.","url":null})).into_response()
    }
}
pub async fn input(Path(source):Path<String>,Json(body):Json<InputBody>)->Response{
    let Some(login)=logins().lock().await.get(&source).cloned()else{
        return error(StatusCode::NOT_FOUND,"진행 중인 로그인이 없습니다.");
    };
    if body.input.len()>512||body.input.contains('\0'){
        return error(StatusCode::BAD_REQUEST,"입력 형식이 올바르지 않습니다.");
    }
    let data=match body.input.as_str(){
        "enter"=>"\r".into(),"down"=>"\u{1b}[B".into(),"up"=>"\u{1b}[A".into(),
        "escape"=>"\u{1b}".into(),value=>format!("{value}\r")
    };
    let mut guard=login.input.lock().await;
    let Some(writer)=guard.as_mut()else{
        return error(StatusCode::CONFLICT,"로그인 프로세스가 종료됐습니다.");
    };
    match writer.write_all(data.as_bytes()).await{
        Ok(())=>Json(json!({"ok":true})).into_response(),
        Err(e)=>error(StatusCode::BAD_GATEWAY,&format!("입력 전달 실패: {e}"))
    }
}
pub async fn cancel(Path(source):Path<String>)->Response{
    let Some(login)=logins().lock().await.get(&source).cloned()else{
        return error(StatusCode::NOT_FOUND,"진행 중인 로그인이 없습니다.");
    };
    if let Some(tx)=login.cancel.lock().await.take(){let _=tx.send(());}
    Json(json!({"ok":true})).into_response()
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn commands_reject_arbitrary_shell_input(){
        assert!(command("claude",None).is_ok());
        assert_eq!(command("opencode",Some("anthropic")).unwrap().1.last().unwrap(),"anthropic");
        assert!(command("opencode",Some("openai;rm")).is_err());
        assert!(command("../../bin/bash",None).is_err());
    }
    #[test]
    fn authorization_url_is_https_and_nonlocal(){
        assert_eq!(authorization_url("Login: https://accounts.google.com/o/oauth2/v2/auth?state=x").unwrap(),
            "https://accounts.google.com/o/oauth2/v2/auth?state=x");
        assert!(authorization_url("http://example.com").is_none());
        assert!(authorization_url("https://127.0.0.1:4317").is_none());
    }
}
