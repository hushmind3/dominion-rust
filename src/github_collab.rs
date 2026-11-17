//! Connected GitHub repository collaboration. All writes pass through engine approval and durable intent.
use crate::{config::Settings,domain::ToolCall,mcp::McpCallOutcome};
use reqwest::{Client,StatusCode,Url};
use serde_json::{json,Value};
use std::time::Duration;
const API:&str="https://api.github.com";
fn def(name:&str,description:&str,properties:Value,required:&[&str])->Value{
 json!({"type":"function","function":{"name":name,"description":description,
 "parameters":{"type":"object","properties":properties,"required":required}}})
}
pub fn definitions()->Vec<Value>{
 let r=json!({"owner":{"type":"string"},"repo":{"type":"string"}});
 let mut issue=r.clone();issue["title"]=json!({"type":"string"});issue["body"]=json!({"type":"string"});
 let mut detail=r.clone();detail["number"]=json!({"type":"integer"});
 let mut comment=detail.clone();comment["body"]=json!({"type":"string"});
 let mut branch=r.clone();for key in ["base_branch","new_branch","expected_sha"]{branch[key]=json!({"type":"string"});}
 let mut pull=issue.clone();for key in ["head","base"]{pull[key]=json!({"type":"string"});}
 pull["draft"]=json!({"type":"boolean"});
 vec![
 def("github_list_branches","List branches and observed commit SHAs for a connected repository.",r.clone(),&["owner","repo"]),
 def("github_create_branch","Create an explicit branch from an observed base SHA; approval and read-back required.",branch,&["owner","repo","base_branch","new_branch","expected_sha"]),
 def("github_list_issues","List open GitHub issues.",r.clone(),&["owner","repo"]),
 def("github_get_issue","Read an issue title, body and state.",detail.clone(),&["owner","repo","number"]),
 def("github_create_issue","Create a GitHub issue and confirm its remote content after approval.",issue,&["owner","repo","title","body"]),
 def("github_comment_issue","Comment on an issue or PR after approval and verify remote comment.",comment,&["owner","repo","number","body"]),
 def("github_list_pull_requests","List open pull requests.",r.clone(),&["owner","repo"]),
 def("github_get_pull_request","Read a pull request's head/base and description.",detail,&["owner","repo","number"]),
 def("github_create_pull_request","Open a PR from explicit head to explicit base after approval, then verify.",pull,&["owner","repo","title","head","base","body"])
 ]
}
pub fn supports(name:&str)->bool{matches!(name,
 "github_list_branches"|"github_create_branch"|"github_list_issues"|"github_get_issue"|
 "github_create_issue"|"github_comment_issue"|"github_list_pull_requests"|
 "github_get_pull_request"|"github_create_pull_request")}
pub fn mutates(name:&str)->bool{matches!(name,
 "github_create_branch"|"github_create_issue"|"github_comment_issue"|"github_create_pull_request")}
fn arg<'a>(a:&'a Value,key:&str)->Result<&'a str,String>{
 a.get(key).and_then(Value::as_str).filter(|s|!s.is_empty()&&s.len()<=32*1024)
 .ok_or_else(||format!("GitHub {key} is required and must be <=32 KiB"))
}
fn slug(s:&str)->Result<&str,String>{
 if s.is_empty()||s.len()>100||s=="."||s==".."||s.ends_with(".git")
 ||!s.bytes().all(|c|c.is_ascii_alphanumeric()||b"._-".contains(&c)){
 return Err("invalid GitHub owner/repository".into());}Ok(s)
}
fn branch(s:&str)->Result<&str,String>{
 if s.is_empty()||s.len()>200||s.contains("..")||s.contains("@{")
 ||s.ends_with(".lock")||s.split('/').any(|p|p.is_empty()||p.starts_with('.')
  ||p.ends_with('.')||!p.bytes().all(|c|c.is_ascii_alphanumeric()||b"._-".contains(&c))){
 return Err("invalid explicit GitHub branch name".into());}Ok(s)
}
fn resource(a:&Value)->Result<(&str,&str),String>{Ok((slug(arg(a,"owner")?)?,slug(arg(a,"repo")?)?))}
fn issue_number(a:&Value)->Result<String,String>{
 a["number"].as_u64().filter(|n|*n>0&&*n<1_000_000_000).map(|n|n.to_string())
 .ok_or("valid issue/PR number required".into())
}
fn url(base:&str,parts:&[&str],query:&[(&str,&str)])->Result<Url,String>{
 let mut u=Url::parse(base).map_err(|e|e.to_string())?;
 {let mut p=u.path_segments_mut().map_err(|_|"invalid GitHub API URL")?;p.clear();p.extend(parts.iter().copied());}
 u.query_pairs_mut().extend_pairs(query.iter().copied());Ok(u)
}
fn client()->Result<Client,String>{
 Client::builder().timeout(Duration::from_secs(20))
 .redirect(reqwest::redirect::Policy::none()).build().map_err(|e|e.to_string())
}
async fn get(http:&Client,token:&str,u:Url)->Result<Value,String>{
 let response=http.get(u).header("User-Agent","Dominion-Rust-GitHub")
 .header("Accept","application/vnd.github+json").bearer_auth(token).send().await
 .map_err(|e|e.to_string())?;
 if !response.status().is_success(){return Err(format!("GitHub read HTTP {}",response.status()));}
 response.json().await.map_err(|e|e.to_string())
}
async fn send_write(http:&Client,token:&str,u:Url,body:Value)->McpCallOutcome{
 let r=http.post(u).header("User-Agent","Dominion-Rust-GitHub")
 .header("Accept","application/vnd.github+json").bearer_auth(token).json(&body).send().await;
 let response=match r{Ok(v)=>v,Err(_)=>return McpCallOutcome::Uncertain(
 "GitHub write transport was interrupted; inspect remote state before reconciling".into())};
 let status=response.status();
 if !status.is_success(){
  if matches!(status.as_u16(),400|401|403|404|409|410|422){
   return McpCallOutcome::Settled(Err(format!("GitHub write HTTP {status}; check permission or conflict")));
  }
  return McpCallOutcome::Uncertain(format!("GitHub write HTTP {status}; inspect remote state"));
 }
 match response.json::<Value>().await{
  Ok(v)=>McpCallOutcome::Settled(Ok(v.to_string())),
  Err(_)=>McpCallOutcome::Uncertain("GitHub write succeeded but response was unreadable".into())
 }
}
fn receipt(v:McpCallOutcome)->Result<Value,McpCallOutcome>{
 match v{McpCallOutcome::Settled(Ok(x))=>
  serde_json::from_str(&x).map_err(|_|McpCallOutcome::Uncertain("GitHub receipt invalid".into())),
  other=>Err(other)}
}
async fn verified_item(http:&Client,token:&str,base:&str,owner:&str,repo:&str,
 kind:&str,wrote:McpCallOutcome,expected:&Value)->McpCallOutcome{
 let result=match receipt(wrote){Ok(v)=>v,Err(x)=>return x};
 let number=match result["number"].as_u64(){Some(n)=>n.to_string(),None=>
  return McpCallOutcome::Uncertain("GitHub created item without number".into())};
 let dest=match url(base,&["repos",owner,repo,kind,&number],&[]){Ok(u)=>u,Err(e)=>return McpCallOutcome::Uncertain(e)};
 match get(http,token,dest).await{
  Ok(v) if v["title"]==expected["title"] && (kind=="pulls"&&
    v["head"]["ref"]==expected["head"]&&v["base"]["ref"]==expected["base"]
    ||kind=="issues"&&v["body"]==expected["body"])=>
    McpCallOutcome::Settled(Ok(json!({"verified":true,"number":number,
      "title":v["title"],"html_url":v["html_url"]}).to_string())),
  _=>McpCallOutcome::Uncertain("GitHub item created but post-write verification failed".into())
 }
}
async fn execute_with(token:&str,base:&str,call:&ToolCall)->McpCallOutcome{
 let http=match client(){Ok(c)=>c,Err(e)=>return McpCallOutcome::Settled(Err(e))};
 let a=&call.arguments;
 let (owner,repo)=match resource(a){Ok(v)=>v,Err(e)=>return McpCallOutcome::Settled(Err(e))};
 let loc=|part:&[&str],q:&[(&str,&str)]|url(base,part,q);
 match call.name.as_str(){
  "github_list_branches"|"github_list_issues"|"github_list_pull_requests"=>{
   let kind=match call.name.as_str(){"github_list_branches"=>"branches",
    "github_list_issues"=>"issues",_=>"pulls"};
   let u=match loc(&["repos",owner,repo,kind],&[("per_page","30"),("state","open")]){
    Ok(u)=>u,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   McpCallOutcome::Settled(get(&http,token,u).await.and_then(|v|{
    let arr=v.as_array().ok_or("GitHub listing is not an array")?;
    let rows=arr.iter().take(30).map(|v|match kind{
     "branches"=>json!({"name":v["name"],"sha":v["commit"]["sha"]}),
     "issues"=>json!({"number":v["number"],"title":v["title"],"html_url":v["html_url"]}),
     _=>json!({"number":v["number"],"title":v["title"],"head":v["head"]["ref"],
       "base":v["base"]["ref"],"html_url":v["html_url"]})
    }).collect::<Vec<_>>();Ok(json!({"items":rows}).to_string())}))
  },
  "github_get_issue"|"github_get_pull_request"=>{
   let n=match issue_number(a){Ok(n)=>n,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   let kind=if call.name=="github_get_issue"{"issues"}else{"pulls"};
   let u=match loc(&["repos",owner,repo,kind,&n],&[]){Ok(u)=>u,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   McpCallOutcome::Settled(get(&http,token,u).await.map(|v|json!({
    "number":v["number"],"title":v["title"],"body":v["body"],
    "head":v["head"]["ref"],"base":v["base"]["ref"],
    "state":v["state"],"html_url":v["html_url"]}).to_string()))
  },
  "github_create_branch"=>{
   let base_branch=match arg(a,"base_branch").and_then(branch){Ok(v)=>v,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   let new_branch=match arg(a,"new_branch").and_then(branch){Ok(v)=>v,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   let sha=match arg(a,"expected_sha"){Ok(v)=>v,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   if sha.len()!=40||!sha.bytes().all(|c|c.is_ascii_hexdigit())||base_branch==new_branch{
    return McpCallOutcome::Settled(Err("distinct branch and observed 40-byte SHA required".into()));
   }
   let mut src=vec!["repos",owner,repo,"git","ref","heads"];src.extend(base_branch.split('/'));
   let u=match loc(&src,&[]){Ok(v)=>v,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   let existing=match get(&http,token,u).await{Ok(v)=>v,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   if existing.pointer("/object/sha").and_then(Value::as_str)!=Some(sha){
    return McpCallOutcome::Settled(Err("source SHA changed; read branch again".into()));
   }
   let u=match loc(&["repos",owner,repo,"git","refs"],&[]){Ok(v)=>v,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   let created=send_write(&http,token,u,json!({"ref":format!("refs/heads/{new_branch}"),"sha":sha})).await;
   let written=match receipt(created){Ok(v)=>v,Err(e)=>return e};
   if written.pointer("/object/sha").and_then(Value::as_str)!=Some(sha){
    return McpCallOutcome::Uncertain("GitHub created a branch with unexpected SHA".into());
   }
   let mut target=vec!["repos",owner,repo,"git","ref","heads"];target.extend(new_branch.split('/'));
   match loc(&target,&[]).and_then(|u|Ok(u)){
    Ok(u)=>match get(&http,token,u).await{
     Ok(v) if v.pointer("/object/sha").and_then(Value::as_str)==Some(sha)=>
      McpCallOutcome::Settled(Ok(json!({"verified":true,"branch":new_branch,"sha":sha}).to_string())),
     _=>McpCallOutcome::Uncertain("GitHub branch created but not remotely confirmed".into())},
    Err(e)=>McpCallOutcome::Uncertain(e)
   }
  },
  "github_create_issue"|"github_create_pull_request"=>{
   let title=match arg(a,"title"){Ok(v) if v.len()<=250=>v,_=>
    return McpCallOutcome::Settled(Err("GitHub title required (<=250 bytes)".into()))};
   let body=match arg(a,"body"){Ok(v)=>v,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   let kind=if call.name=="github_create_issue"{"issues"}else{"pulls"};
   let mut data=json!({"title":title,"body":body});
   if kind=="pulls"{
    let head=match arg(a,"head").and_then(branch){Ok(v)=>v,Err(e)=>return McpCallOutcome::Settled(Err(e))};
    let base_branch=match arg(a,"base").and_then(branch){Ok(v)=>v,Err(e)=>return McpCallOutcome::Settled(Err(e))};
    if head==base_branch{return McpCallOutcome::Settled(Err("PR head and base must differ".into()));}
    data["head"]=json!(head);data["base"]=json!(base_branch);
    data["draft"]=json!(a["draft"].as_bool().unwrap_or(true));
   }
   let u=match loc(&["repos",owner,repo,kind],&[]){Ok(v)=>v,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   let wrote=send_write(&http,token,u,data.clone()).await;
   verified_item(&http,token,base,owner,repo,kind,wrote,&data).await
  },
  "github_comment_issue"=>{
   let n=match issue_number(a){Ok(n)=>n,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   let body=match arg(a,"body"){Ok(v)=>v,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   let u=match loc(&["repos",owner,repo,"issues",&n,"comments"],&[]){Ok(v)=>v,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   let wrote=send_write(&http,token,u,json!({"body":body})).await;
   let response=match receipt(wrote){Ok(v)=>v,Err(e)=>return e};
   let id=match response["id"].as_u64(){Some(v)=>v.to_string(),None=>
    return McpCallOutcome::Uncertain("GitHub comment lacks receipt ID".into())};
   let u=match loc(&["repos",owner,repo,"issues","comments",&id],&[]){Ok(v)=>v,Err(e)=>return McpCallOutcome::Uncertain(e)};
   match get(&http,token,u).await{
    Ok(v) if v["body"]==body=>McpCallOutcome::Settled(Ok(json!({"verified":true,
      "comment_id":id,"html_url":v["html_url"]}).to_string())),
    _=>McpCallOutcome::Uncertain("GitHub comment sent but remote confirmation failed".into())
   }
  },
  _=>McpCallOutcome::Settled(Err("unsupported GitHub collaboration tool".into()))
 }
}
pub async fn execute(settings:&Settings,call:&ToolCall)->McpCallOutcome{
 let token=match crate::frontend_oauth::github_access_token(settings){
  Ok(v)=>v,Err(e)=>return McpCallOutcome::Settled(Err(e))
 };
 execute_with(&token,API,call).await
}
#[cfg(test)]
mod tests{
 use super::*;
 use axum::{Router,Json,routing::{get,post},extract::State};
 use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
 #[test]fn mutating_tools_are_explicit_and_branch_names_are_validated(){
  assert!(supports("github_create_pull_request")&&mutates("github_create_pull_request"));
  assert!(supports("github_list_issues")&&!mutates("github_list_issues"));
  assert!(!supports("github_delete_repository"));
  assert!(branch("feature/new").is_ok());assert!(branch("../main").is_err());
  assert!(slug("owner").is_ok());assert!(slug("http://evil").is_err());
 }
 #[derive(Clone)]struct Fixture(Arc<AtomicUsize>);
 async fn create(State(f):State<Fixture>,Json(v):Json<Value>)->Json<Value>{
  assert_eq!(v["title"],"Regression");
  f.0.fetch_add(1,Ordering::SeqCst);
  Json(json!({"number":17}))
 }
 async fn read_issue()->Json<Value>{
  Json(json!({"number":17,"title":"Regression","body":"Details","html_url":"https://github.com/example/repo/issues/17"}))
 }
 #[tokio::test]async fn create_issue_verifies_remote_receipt(){
  let fixture=Fixture(Arc::new(AtomicUsize::new(0)));
  let router=Router::new().route("/repos/example/repo/issues",post(create))
   .route("/repos/example/repo/issues/17",get(read_issue)).with_state(fixture.clone());
  let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let addr=listener.local_addr().unwrap();
  tokio::spawn(async move{axum::serve(listener,router).await.unwrap();});
  let call=ToolCall{id:"i1".into(),name:"github_create_issue".into(),
   arguments:json!({"owner":"example","repo":"repo","title":"Regression","body":"Details"})};
  match execute_with("fake",&format!("http://{addr}"),&call).await{
   McpCallOutcome::Settled(Ok(v))=>assert!(v.contains("\"verified\":true")),
   _=>panic!("expected verified GitHub issue"),
  }
  assert_eq!(fixture.0.load(Ordering::SeqCst),1);
 }
}
