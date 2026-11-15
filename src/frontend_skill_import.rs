//! Explicit GitHub SKILL.md imports for the original ExtensionsPage.
//! GitHub is a source repository, not an executor or trusted system prompt.
//! Each imported instruction is pinned to an actual commit and SHA-256 digest.
use crate::{engine::Engine,frontend_extensions::ManagedSkill};
use axum::{extract::State,http::StatusCode,routing::post,Json,Router};
use serde::Deserialize;
use serde_json::{json,Value};
use sha2::{Sha256,Digest};
use std::{path::{Component,Path},sync::Arc,time::Duration};
use uuid::Uuid;
type Fault=(StatusCode,Json<Value>);
fn bad(message:impl Into<String>)->Fault{
 (StatusCode::BAD_REQUEST,Json(json!({"error":message.into()})))
}
fn fail(message:impl Into<String>)->Fault{
 (StatusCode::BAD_GATEWAY,Json(json!({"error":message.into()})))
}
const MAX_SKILLS:usize=12;
const MAX_BYTES:usize=30_000;
fn repo_id(text:&str)->Result<(String,String),String>{
 let raw=text.trim().trim_end_matches('/');
 let short=if let Some(source)=raw.strip_prefix("https://github.com/"){source}else{raw};
 let short=short.trim_end_matches(".git");
 let mut parts=short.split('/');
 let owner=parts.next().unwrap_or("");
 let repo=parts.next().unwrap_or("");
 if parts.next().is_some()||owner.len()>100||repo.len()>100||
  owner.is_empty()||repo.is_empty()||[owner,repo].iter().any(|part|
  !part.bytes().all(|c|c.is_ascii_alphanumeric()||c==b'-'||c==b'_'||c==b'.')){
  return Err("provide a GitHub owner/repo or https://github.com/owner/repo".into());
 }
 Ok((owner.into(),repo.into()))
}
fn safe_skill(path:&str)->bool{
 if path.len()>400||!path.ends_with("/SKILL.md")&&path!="SKILL.md"{return false;}
 if path.split('/').any(|c|matches!(c,".git"|".github"|"node_modules"|
  "vendor"|"target"|"dist"|"build"|"tests"|"test"|"fixtures")){return false;}
 let p=Path::new(path);
 p.components().all(|c|matches!(c,Component::Normal(_)))
}
fn name_from_source(content:&str,path:&str)->String{
 let named=content.lines().find_map(|line|line.strip_prefix("# ")
  .map(|title|title.trim().chars().take(150).collect::<String>()))
  .filter(|title|!title.is_empty());
 named.unwrap_or_else(||{
  Path::new(path).parent().and_then(|p|p.file_name()).and_then(|name|name.to_str())
  .unwrap_or("Imported Skill").to_owned()
 })
}
fn api_url(parts:&[&str])->Result<reqwest::Url,String>{
 let mut url=reqwest::Url::parse("https://api.github.com").map_err(|e|e.to_string())?;
 url.path_segments_mut().map_err(|_|"invalid GitHub path")?
  .extend(parts.iter().copied());
 Ok(url)
}
fn raw_url(owner:&str,repo:&str,commit:&str,path:&str)->Result<reqwest::Url,String>{
 let mut url=reqwest::Url::parse("https://raw.githubusercontent.com/")
  .map_err(|e|e.to_string())?;
 url.path_segments_mut().map_err(|_|"invalid raw GitHub path")?
  .extend([owner,repo,commit].into_iter().chain(path.split('/')));
 Ok(url)
}
fn http()->Result<reqwest::Client,String>{
 reqwest::Client::builder().timeout(Duration::from_secs(15))
  .redirect(reqwest::redirect::Policy::none()).build().map_err(|e|e.to_string())
}
async fn fetch(http:&reqwest::Client,url:reqwest::Url,limit:usize)->Result<Vec<u8>,String>{
 let mut request=http.get(url).header("User-Agent","Dominion-Rust-Skill-Importer")
  .header("Accept","application/vnd.github+json");
 if let Ok(token)=std::env::var("DOMINION_GITHUB_TOKEN"){
  if !token.is_empty(){request=request.bearer_auth(token);}
 }
 let response=request.send().await.map_err(|e|format!("GitHub source fetch: {e}"))?;
 if !response.status().is_success(){
  return Err(format!("GitHub source HTTP {}",response.status()));
 }
 if response.content_length().is_some_and(|bytes|bytes>limit as u64){
  return Err("GitHub source exceeds bounded import size".into());
 }
 let bytes=response.bytes().await.map_err(|e|e.to_string())?;
 if bytes.len()>limit{return Err("GitHub source exceeds bounded import size".into());}
 Ok(bytes.to_vec())
}
fn sha(input:&[u8])->String{
 format!("{:x}",Sha256::digest(input))
}
async fn import_public(engine:&Arc<Engine>,requested:&str)->Result<Value,String>{
 let (owner,repo)=repo_id(requested)?;
 let http=http()?;
 let info=fetch(&http,api_url(&["repos",&owner,&repo])?,64_000).await?;
 let meta:Value=serde_json::from_slice(&info).map_err(|e|e.to_string())?;
 if meta["private"]!=false{return Err("GitHub repository must be a readable public repository".into());}
 let default_branch=meta["default_branch"].as_str().ok_or("repository has no default branch")?;
 let commit=fetch(&http,api_url(&["repos",&owner,&repo,"commits",default_branch])?,120_000).await?;
 let revision:Value=serde_json::from_slice(&commit).map_err(|e|e.to_string())?;
 let commit=revision["sha"].as_str().ok_or("GitHub commit SHA unavailable")?;
 if commit.len()!=40||!commit.bytes().all(|b|b.is_ascii_hexdigit()){
  return Err("GitHub did not provide an immutable commit SHA".into());
 }
 let tree=fetch(&http,api_url(&["repos",&owner,&repo,"git","trees",commit])?
  .join("?recursive=1").map_err(|e|e.to_string())?,2_000_000).await?;
 let tree:Value=serde_json::from_slice(&tree).map_err(|e|e.to_string())?;
 if tree["truncated"]==true{return Err("repository tree is incomplete; import a smaller source".into());}
 let manifest=tree["tree"].as_array().ok_or("GitHub tree missing files")?;
 let paths=manifest.iter().filter_map(|v|{
  let p=v["path"].as_str()?;
  if v["type"]!="blob"||!safe_skill(p){return None;}
  Some(p.to_owned())
 }).collect::<Vec<_>>();
 if paths.len()>MAX_SKILLS{return Err("repository contains more than 12 SKILL.md modules".into());}
 if paths.is_empty(){return Err("repository contains no SKILL.md files".into());}
 let mut prepared=Vec::new();
 for path in paths{
  let url=raw_url(&owner,&repo,commit,&path)?;
  let bytes=fetch(&http,url,MAX_BYTES).await?;
  let source_text=String::from_utf8(bytes.clone())
   .map_err(|_|format!("{path} must be UTF-8 markdown"))?;
  if source_text.trim().is_empty(){return Err(format!("{path} is empty"));}
  let fingerprint=sha(&bytes);
  let source=format!("github:{owner}/{repo}@{commit}:{path}#sha256={fingerprint}");
  let name=name_from_source(&source_text,&path);
  prepared.push(ManagedSkill{
   id:Uuid::new_v4().to_string(),name,instructions:source_text,
   enabled:true,source,
  });
 }
 let mut added=0;
 let existing=engine.extensions.skill_list().await;
 let sources=existing.iter().map(|s|s.source.clone()).collect::<std::collections::HashSet<_>>();
 for item in prepared {
  if sources.contains(&item.source){continue;}
  engine.extensions.upsert_skill(item,false).await?;
  added+=1;
 }
 Ok(json!({"repo":format!("{owner}/{repo}"),"commit":commit,
  "added":added,"skipped":sources.len().min(MAX_SKILLS)}))
}
#[derive(Deserialize)]
struct ImportRepo{repo:String}
#[derive(Deserialize)]
struct ImportCatalog{id:String}
pub fn routes()->Router<Arc<Engine>>{
 Router::new().route("/skills/import-repo",post(import_repo))
  .route("/skill-catalog/import",post(import_catalog))
}
async fn import_repo(State(engine):State<Arc<Engine>>,Json(input):Json<ImportRepo>)
 ->Result<Json<Value>,Fault>{
 import_public(&engine,&input.repo).await.map(Json).map_err(fail)
}
async fn import_catalog(State(engine):State<Arc<Engine>>,Json(input):Json<ImportCatalog>)
 ->Result<Json<Value>,Fault>{
 // Catalog IDs must be explicit repository selectors; arbitrary URLs,
 // generated code and installer commands are not accepted.
 repo_id(&input.id).map_err(bad)?;
 import_public(&engine,&input.id).await.map(Json).map_err(fail)
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test]
 fn source_paths_cannot_escape_expected_markdown_scope(){
  assert!(safe_skill("skills/review/SKILL.md"));
  assert!(!safe_skill("../other/SKILL.md"));
  assert!(!safe_skill("skills/test/SKILL.md"));
  assert!(!safe_skill("install.sh"));
  assert_eq!(repo_id("https://github.com/example/skills.git").unwrap(),
   ("example".to_owned(),"skills".to_owned()));
  assert!(repo_id("https://not-github.example/repo").is_err());
  assert_eq!(sha(b"abc"),"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
 }
}
