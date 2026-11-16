//! Native, scope-bound GitHub repository operations. No arbitrary REST URL,
//! MCP credential forwarding or user/model supplied OAuth tokens.
//! Mutations run only behind the engine's durable human approval boundary.
use crate::{config::Settings,domain::ToolCall,mcp::McpCallOutcome};
use base64::{Engine as _,engine::general_purpose::STANDARD};
use reqwest::{Client,StatusCode,Url};
use serde_json::{json,Value};
use std::time::Duration;

const API:&str="https://api.github.com";
const CONTENT_LIMIT:usize=256*1024;

pub fn definitions()->Vec<Value>{
 vec![
  tool("github_list_repositories",
    "List repositories available to the connected GitHub identity; choose owner/repo and branch before writing.",
    json!({"type":"object","properties":{"page":{"type":"integer","minimum":1,"maximum":10}}})),
  tool("github_read_file",
    "Read a GitHub file from an explicitly named owner/repository and branch, returning its SHA for a safe update.",
    json!({"type":"object","properties":{
      "owner":{"type":"string"},"repo":{"type":"string"},"path":{"type":"string"},
      "branch":{"type":"string"}
    },"required":["owner","repo","path","branch"]})),
  tool("github_commit_file",
    "After explicit human approval, replace one existing GitHub file using an observed blob SHA; commit, then re-fetch remote bytes to verify. Never edit a default branch implicitly.",
    json!({"type":"object","properties":{
      "owner":{"type":"string"},"repo":{"type":"string"},"path":{"type":"string"},
      "branch":{"type":"string"},"expected_sha":{"type":"string"},
      "content":{"type":"string"},"message":{"type":"string"}
    },"required":["owner","repo","path","branch","expected_sha","content","message"]}))
 ]
}
fn tool(name:&str,description:&str,parameters:Value)->Value{
 json!({"type":"function","function":{"name":name,"description":description,"parameters":parameters}})
}
fn text<'a>(v:&'a Value,key:&str)->Result<&'a str,String>{
 v.get(key).and_then(Value::as_str)
  .ok_or_else(||format!("GitHub {key} required"))
}
fn branch_ref(value:&str)->Result<&str,String>{
 if value.is_empty()||value.len()>180||value.starts_with('/')||value.ends_with('/')
     ||value.contains("..")||value.contains("//")||
     !value.bytes().all(|b|b.is_ascii_alphanumeric()||b"-_./".contains(&b)){
    return Err("GitHub branch must be an explicit safe ref name".into());
 }
 Ok(value)
}
fn component(value:&str)->Result<&str,String>{
 if value.is_empty()||value.len()>120||value.starts_with('.')||
    !value.bytes().all(|b|b.is_ascii_alphanumeric()||b"-_.".contains(&b)){
    return Err("GitHub owner, repository or branch contains an invalid character".into());
 }
 Ok(value)
}
fn file_path(path:&str)->Result<Vec<&str>,String>{
 if path.len()>640||path.is_empty()||path.starts_with('/')||
    path.bytes().any(|b|b<0x20||b==b'\\'){
    return Err("GitHub file path invalid or too long".into());
 }
 let parts=path.split('/').collect::<Vec<_>>();
 if parts.iter().any(|s|s.is_empty()||*s=="."||*s==".."||s.starts_with('.')){
    // Explicitly block .github workflow modifications: workflow grants
    // are independent from repository Contents permission.
    return Err("GitHub path contains an unsafe or hidden component".into());
 }
 Ok(parts)
}
fn link(base:&str,parts:&[&str],params:&[(&str,&str)])->Result<Url,String>{
 let mut url=Url::parse(base).map_err(|e|e.to_string())?;
 url.path_segments_mut().map_err(|_|"invalid GitHub API origin")?
    .extend(parts.iter().copied());
 if !params.is_empty(){url.query_pairs_mut().extend_pairs(params.iter().copied());}
 Ok(url)
}
fn client()->Result<Client,String>{
 Client::builder().timeout(Duration::from_secs(30))
    .redirect(reqwest::redirect::Policy::none())
    .build().map_err(|e|e.to_string())
}
fn request(http:&Client,token:&str,url:Url)->reqwest::RequestBuilder{
 http.get(url).header("Accept","application/vnd.github+json")
    .header("User-Agent","Dominion-Rust-GitHub").bearer_auth(token)
}
fn reconnect(status:StatusCode)->String{
 if status==StatusCode::UNAUTHORIZED{
    "GitHub authentication expired or revoked; reconnect the account".into()
 }else if status==StatusCode::FORBIDDEN{
    "GitHub repository permission denied; verify repository access and reconnect with Contents permission".into()
 }else{
    format!("GitHub returned HTTP {status}; verify owner/repository, branch and permissions")
 }
}
fn repo_parts<'a>(a:&'a Value)->Result<(&'a str,&'a str,&'a str),String>{
 Ok((component(text(a,"owner")?)?,component(text(a,"repo")?)?,
     branch_ref(text(a,"branch")?)?))
}
async fn list_repositories(http:&Client,token:&str,base:&str,args:&Value)->Result<String,String>{
 let page=args.get("page").and_then(Value::as_u64).unwrap_or(1);
 if !(1..=10).contains(&page){return Err("GitHub page must be 1..10".into());}
 let page_str=page.to_string();
 let url=link(base,&["user","repos"],&[("per_page","30"),("page",&page_str),("sort","updated")])?;
 let response=request(http,token,url).send().await.map_err(|e|e.to_string())?;
 if !response.status().is_success(){return Err(reconnect(response.status()));}
 let entries:Value=response.json().await.map_err(|e|e.to_string())?;
 let rows=entries.as_array().ok_or("GitHub repository list was not an array")?
    .iter().take(30).map(|value|json!({
        "full_name":value["full_name"],"private":value["private"],
        "default_branch":value["default_branch"],
        "can_push":value["permissions"]["push"],
        "html_url":value["html_url"]
    })).collect::<Vec<_>>();
 Ok(json!({"page":page,"repositories":rows}).to_string())
}
async fn read_file(http:&Client,token:&str,base:&str,args:&Value)->Result<Value,String>{
 let (owner,repo,branch)=repo_parts(args)?;
 let path=file_path(text(args,"path")?)?;
 let mut parts=vec!["repos",owner,repo,"contents"];
 parts.extend(path);
 let url=link(base,&parts,&[("ref",branch)])?;
 let response=request(http,token,url).send().await.map_err(|e|e.to_string())?;
 if !response.status().is_success(){return Err(reconnect(response.status()));}
 let raw:Value=response.json().await.map_err(|e|e.to_string())?;
 if raw["type"]!="file" || raw["encoding"]!="base64"{
    return Err("GitHub entry is not a Base64 file; choose a regular file".into());
 }
 let sha=raw["sha"].as_str().ok_or("GitHub did not return file SHA")?;
 let bytes=raw["content"].as_str().ok_or("GitHub file payload missing")?
    .bytes().filter(|b|!b.is_ascii_whitespace()).collect::<Vec<_>>();
 if bytes.len()>CONTENT_LIMIT*4/3+32{
    return Err("GitHub file exceeds the 256 KiB connected-editor limit".into());
 }
 let data=STANDARD.decode(&bytes).map_err(|_|"GitHub returned invalid Base64 file data")?;
 if data.len()>CONTENT_LIMIT{return Err("GitHub file exceeds 256 KiB".into());}
 let body=String::from_utf8(data).map_err(|_|"GitHub file is not UTF-8 text")?;
 Ok(json!({
    "owner":owner,"repo":repo,"path":text(args,"path")?,
    "branch":branch,"sha":sha,"content":body,
    "html_url":raw["html_url"],"size_bytes":body.len()
 }))
}
async fn commit_file(http:&Client,token:&str,base:&str,args:&Value)->McpCallOutcome{
 let result=commit_prepare(http,token,base,args).await;
 let (url,body,old_sha,owner,repo,branch,path)=match result{
   Ok(value)=>value,
   Err(error)=>return McpCallOutcome::Settled(Err(error))
 };
 // The conditional SHA protects against overwrite of a remote update.
 // A lost transport reply after this send is an uncertain remote effect.
 let sent=http.put(url)
    .header("Accept","application/vnd.github+json")
    .header("User-Agent","Dominion-Rust-GitHub")
    .bearer_auth(token).json(&body).send().await;
 let response=match sent {
   Ok(response)=>response,
   Err(_)=>return McpCallOutcome::Uncertain(
       "GitHub file update response was lost; inspect the remote branch before reconciling".into())
 };
 let status=response.status();
 if status==StatusCode::CONFLICT || status==StatusCode::UNPROCESSABLE_ENTITY
     || status==StatusCode::FORBIDDEN || status==StatusCode::UNAUTHORIZED
     || status==StatusCode::NOT_FOUND{
     return McpCallOutcome::Settled(Err(reconnect(status)));
 }
 if !status.is_success(){
   return McpCallOutcome::Uncertain(
       format!("GitHub update returned HTTP {status}; verify remote file before retrying"))
 }
 let receipt:Value=match response.json().await{
   Ok(value)=>value,
   Err(_)=>return McpCallOutcome::Uncertain(
       "GitHub update returned malformed receipt; inspect remote commit before reconciling".into())
 };
 let new_sha=receipt["content"]["sha"].as_str().unwrap_or("");
 let commit_sha=receipt["commit"]["sha"].as_str().unwrap_or("");
 if new_sha.is_empty()||commit_sha.is_empty(){
   return McpCallOutcome::Uncertain(
      "GitHub update omitted a verifiable commit SHA; inspect the remote branch".into())
 }
 let check=json!({"owner":owner,"repo":repo,"path":path,"branch":branch});
 match read_file(http,token,base,&check).await {
   Ok(found) if found["sha"]==new_sha && found["content"]==args["content"]=>{
     McpCallOutcome::Settled(Ok(json!({
       "verified":true,"owner":owner,"repo":repo,"path":path,"branch":branch,
       "previous_sha":old_sha,"sha":new_sha,"commit_sha":commit_sha,
       "html_url":receipt["commit"]["html_url"]
     }).to_string()))
   },
   _=>McpCallOutcome::Uncertain(
       "GitHub accepted the commit but post-commit remote verification failed; reconcile the observed branch state".into())
 }
}
type Prepared=(Url,Value,String,String,String,String,String);
async fn commit_prepare(http:&Client,token:&str,base:&str,args:&Value)->Result<Prepared,String>{
 let (owner,repo,branch)=repo_parts(args)?;
 let path=text(args,"path")?;
 let file=file_path(path)?;
 let content=text(args,"content")?;
 let message=text(args,"message")?;
 let expected=text(args,"expected_sha")?;
 if content.len()>CONTENT_LIMIT{return Err("GitHub edit exceeds 256 KiB".into());}
 if message.trim().is_empty()||message.len()>250 || message.contains('\n'){
    return Err("GitHub commit message must be one line, 1..250 bytes".into());
 }
 if expected.len()!=40||!expected.bytes().all(|b|b.is_ascii_hexdigit()){
    return Err("GitHub expected_sha must be an observed 40-character hex SHA".into());
 }
 // GitHub explicitly exposes the authenticated user's push permission for a
 // repository. A read-only collaboration grant must not be used for commits.
 let permissions_url=link(base,&["repos",owner,repo],&[])?;
 let response=request(http,token,permissions_url).send().await.map_err(|e|e.to_string())?;
 if !response.status().is_success(){return Err(reconnect(response.status()));}
 let data:Value=response.json().await.map_err(|e|e.to_string())?;
 if data["permissions"]["push"]!=true{
   return Err("GitHub account has no push permission for the selected repository".into());
 }
 let current=read_file(http,token,base,args).await?;
 if current["sha"]!=expected{
   return Err("GitHub remote SHA changed; read the file again before editing".into());
 }
 let mut segments=vec!["repos",owner,repo,"contents"];
 segments.extend(file);
 let url=link(base,&segments,&[])?;
 let body=json!({"message":message,"content":STANDARD.encode(content.as_bytes()),
     "sha":expected,"branch":branch});
 Ok((url,body,expected.into(),owner.into(),repo.into(),branch.into(),path.into()))
}
pub async fn execute(settings:&Settings,call:&ToolCall)->McpCallOutcome{
 let token=match crate::frontend_oauth::github_access_token(settings){
   Ok(token)=>token,
   Err(error)=>return McpCallOutcome::Settled(Err(error))
 };
 execute_with(&token,API,call).await
}
async fn execute_with(token:&str,base:&str,call:&ToolCall)->McpCallOutcome{
 let http=match client(){
   Ok(client)=>client,
   Err(e)=>return McpCallOutcome::Settled(Err(e))
 };
 match call.name.as_str(){
  "github_list_repositories"=>
    McpCallOutcome::Settled(list_repositories(&http,token,base,&call.arguments).await),
  "github_read_file"=>
    McpCallOutcome::Settled(read_file(&http,token,base,&call.arguments).await.map(|v|v.to_string())),
  "github_commit_file"=>commit_file(&http,token,base,&call.arguments).await,
  _=>McpCallOutcome::Settled(Err("unsupported GitHub tool".into()))
 }
}
#[cfg(test)]
mod tests{
 use super::*;
 use axum::{extract::{Path,State},routing::{get,put},http::{StatusCode as AxumStatus,HeaderMap},Json,Router};
 use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
 #[test]
 fn github_paths_require_explicit_branch_and_reject_workflow_edits(){
    assert!(file_path("src/main.rs").is_ok());
    assert!(file_path(".github/workflows/check.yml").is_err());
    assert!(file_path("../secrets").is_err());
    assert!(file_path("src//main.rs").is_err());
    assert!(component("main").is_ok());
    assert!(component("release/v1").is_err());
    assert!(branch_ref("feature/fix-ui").is_ok());
    assert!(branch_ref("../main").is_err());
    assert!(repo_parts(&json!({"owner":"alice","repo":"code"})).is_err());
 }
 #[derive(Clone)]struct Fixture {writes:Arc<AtomicUsize>}
 async fn repo(State(_fixture):State<Fixture>,headers:HeaderMap)->Json<Value>{
    assert_eq!(headers.get("authorization").unwrap(),"Bearer fake-token");
    Json(json!({"permissions":{"push":true}}))
 }
 async fn file(State(fixture):State<Fixture>,Path(_file):Path<String>)->Json<Value>{
    let changed=fixture.writes.load(Ordering::SeqCst)>0;
    let content=if changed{"pub fn fixed() {}"}else{"pub fn broken() {}"};
    Json(json!({
       "type":"file","encoding":"base64",
       "sha":if changed{"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}
             else{"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
       "content":STANDARD.encode(content),"html_url":"https://github.com/alice/code/blob/main/src/main.rs"
    }))
 }
 async fn update(State(fixture):State<Fixture>,Json(body):Json<Value>)->(AxumStatus,Json<Value>){
    assert_eq!(body["sha"],"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    assert_eq!(body["branch"],"main");
    assert_eq!(body["content"],STANDARD.encode("pub fn fixed() {}"));
    fixture.writes.fetch_add(1,Ordering::SeqCst);
    (AxumStatus::OK,Json(json!({
      "content":{"sha":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"},
      "commit":{"sha":"cccccccccccccccccccccccccccccccccccccccc","html_url":"https://github.com/alice/code/commit/ccc"}
    })))
 }
 #[tokio::test]
 async fn connected_repository_edit_returns_verified_remote_receipt(){
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fixture=Fixture{writes:Arc::new(AtomicUsize::new(0))};
    let writes=Arc::clone(&fixture.writes);
    let router=Router::new()
      .route("/repos/alice/code",get(repo))
      .route("/repos/alice/code/contents/{*file}",get(file).put(update))
      .with_state(fixture);
    let host=format!("http://{}",listener.local_addr().unwrap());
    let service=tokio::spawn(async move{axum::serve(listener,router).await.unwrap()});
    let call=ToolCall{id:"commit-1".into(),name:"github_commit_file".into(),
      arguments:json!({"owner":"alice","repo":"code","path":"src/main.rs",
        "branch":"main","expected_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "content":"pub fn fixed() {}","message":"fix test"})};
    let outcome=execute_with("fake-token",&host,&call).await;
    match outcome{
      McpCallOutcome::Settled(Ok(receipt))=>{
         let value:Value=serde_json::from_str(&receipt).unwrap();
         assert_eq!(value["verified"],true);
         assert_eq!(value["sha"],"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
      },
      _=>panic!("GitHub fixture did not verify its remote commit")
    }
    assert_eq!(writes.load(Ordering::SeqCst),1);
    service.abort();
 }
 #[tokio::test]
 async fn stale_sha_never_dispatches_remote_mutation(){
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fixture=Fixture{writes:Arc::new(AtomicUsize::new(0))};
    let writes=Arc::clone(&fixture.writes);
    let router=Router::new()
      .route("/repos/alice/code",get(repo))
      .route("/repos/alice/code/contents/{*file}",get(file).put(update))
      .with_state(fixture);
    let host=format!("http://{}",listener.local_addr().unwrap());
    let service=tokio::spawn(async move{axum::serve(listener,router).await.unwrap()});
    let call=ToolCall{id:"commit-2".into(),name:"github_commit_file".into(),
      arguments:json!({"owner":"alice","repo":"code","path":"src/main.rs",
        "branch":"main","expected_sha":"dddddddddddddddddddddddddddddddddddddddd",
        "content":"pub fn fixed() {}","message":"fix test"})};
    let outcome=execute_with("fake-token",&host,&call).await;
    assert!(matches!(outcome,McpCallOutcome::Settled(Err(_))));
    assert_eq!(writes.load(Ordering::SeqCst),0);
    service.abort();
 }
}
