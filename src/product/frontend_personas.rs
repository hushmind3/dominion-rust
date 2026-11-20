//! Import user-authored SOUL.md identities into the existing native agent store.
//! This never executes profile files, imports credentials, or mounts an alien engine.
use crate::{engine::Engine,frontend_state::UiAgent};
use axum::{extract::State,http::StatusCode,routing::{get,post},Json,Router};
use base64::{engine::general_purpose::STANDARD,Engine as _};
use serde::Deserialize;
use serde_json::{json,Value};
use std::{path::{Path,PathBuf},sync::Arc};
type Fault=(StatusCode,Json<Value>);
const MAX_SOUL:usize=96*1024;
const MAX_AVATAR:usize=1024*1024;
const PROFILES:[(&str,&str,&str);9]=[
 ("ainz","아인즈","#4B3B73"),("albedo","알베도","#7B56A0"),
 ("shalltear","샤르티아","#AA3154"),("cocytus","코퀴토스","#318CA6"),
 ("demiurge","데미우르고스","#AD4F34"),("aura","아우라","#8AA33A"),
 ("mare","마레","#55885F"),("sebas","세바스","#63707C"),
 ("pandora-actor","판도라 액터","#B58C36"),
];
fn root()->PathBuf{
 std::env::var_os("DOMINION_PERSONAS_DIR").map(PathBuf::from).unwrap_or_else(||{
  PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".dominion/personas")
 })
}
fn bounded(root:&Path,path:&Path,limit:usize)->Result<Vec<u8>,String>{
 let actual=path.canonicalize().map_err(|e|format!("profile file unavailable: {e}"))?;
 if !actual.starts_with(root){return Err("profile path escapes the persona directory".into());}
 let meta=std::fs::metadata(&actual).map_err(|e|e.to_string())?;
 if !meta.is_file()||meta.len()>limit as u64{return Err("profile file exceeds size limit".into());}
 let bytes=std::fs::read(actual).map_err(|e|e.to_string())?;
 if bytes.len()>limit{return Err("profile file exceeds size limit".into());}
 Ok(bytes)
}
#[derive(Clone)]
struct Persona{
 slug:String,name:String,color:String,prompt:String,description:String,role:String,avatar:String,
}
fn load(root:&Path,slug:&str,name:&str,color:&str)->Result<Persona,String>{
 let directory=root.join(slug).canonicalize().map_err(|e|e.to_string())?;
 if !directory.starts_with(root)||!directory.is_dir(){return Err("profile directory is invalid".into());}
 let bytes=bounded(root,&directory.join("SOUL.md"),MAX_SOUL)?;
 let prompt=String::from_utf8(bytes).map_err(|_|"SOUL.md requires UTF-8".to_owned())?;
 if prompt.trim().is_empty(){return Err("SOUL.md is empty".into());}
 let role=prompt.lines().find_map(|line|line.trim().strip_prefix("담당:"))
  .unwrap_or("독립 에이전트").trim().chars().take(120).collect();
 let description=prompt.lines().map(str::trim)
  .find(|line|!line.is_empty()&&!line.starts_with('#'))
  .unwrap_or(name).chars().take(180).collect();
 let avatar_path=directory.join("assets/avatar.png");
 let avatar=if avatar_path.exists(){
  let bytes=bounded(root,&avatar_path,MAX_AVATAR)?;
  if !bytes.starts_with(b"\x89PNG\r\n\x1a\n"){
   return Err("persona avatar is not PNG".into());
  }
  format!("data:image/png;base64,{}",STANDARD.encode(bytes))
 }else{String::new()};
 Ok(Persona{slug:slug.into(),name:name.into(),color:color.into(),prompt,description,role,avatar})
}
fn available()->Vec<(String,Result<Persona,String>)>{
 let root=root();
 let canonical=root.canonicalize();
 PROFILES.iter().map(|(slug,name,color)|{
  ((*slug).to_owned(),match &canonical{
   Ok(base)=>load(base,slug,name,color),
   Err(_)=>Err("persona directory is not installed".into())
  })
 }).collect()
}
pub fn routes()->Router<Arc<Engine>>{
 Router::new().route("/personas/import/preview",get(preview))
  .route("/personas/import",post(import))
}
async fn preview(State(engine):State<Arc<Engine>>)->Json<Value>{
 let existing=engine.frontend.agents().await;
 let profiles=available().into_iter().map(|(slug,source)|{
  let id=format!("dominion-{slug}");
  let present=existing.iter().any(|agent|agent.id==id);
  match source{
   Ok(p)=>json!({"sourceProfile":slug,"id":id,"name":p.name,
    "description":p.description,"role":p.role,"hasAvatar":!p.avatar.is_empty(),
    "personaPreview":p.prompt.chars().take(1600).collect::<String>(),
    "exists":present,"available":true}),
   Err(error)=>json!({"sourceProfile":slug,"id":id,"name":PROFILES.iter()
    .find(|(name,_,_)|*name==slug).map(|(_,name,_)|*name).unwrap_or(""),
    "exists":present,"available":false,"hasAvatar":false,"error":error}),
  }
 }).collect::<Vec<_>>();
 Json(json!({"source":"local-user-profiles",
  "count":profiles.iter().filter(|p|p["available"]==true).count(),
  "profiles":profiles}))
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct Import{apply:bool,profile_ids:Vec<String>,#[serde(default)]refresh_identity:bool}
async fn import(State(engine):State<Arc<Engine>>,Json(body):Json<Import>)
 ->Result<Json<Value>,Fault>{
 if !body.apply{return Err((StatusCode::BAD_REQUEST,Json(json!({"error":"explicit apply required"}))));}
 if body.profile_ids.is_empty()||body.profile_ids.len()>PROFILES.len(){
  return Err((StatusCode::BAD_REQUEST,Json(json!({"error":"select 1-9 personas"}))));
 }
 let mut unique=std::collections::HashSet::new();
 if body.profile_ids.iter().any(|id|!unique.insert(id.clone())||
  !PROFILES.iter().any(|(slug,_,_)|slug==id)){
  return Err((StatusCode::BAD_REQUEST,Json(json!({"error":"invalid or duplicate persona"}))));
 }
 let mut prepared=Vec::new();
 let mut errors=Vec::new();
 for (slug,result) in available(){
  if body.profile_ids.contains(&slug){
   match result {Ok(p)=>prepared.push(p),Err(error)=>errors.push(json!({"sourceProfile":slug,"error":error}))}
  }
 }
 let model=engine.settings.main.name.clone();
 let (imported,skipped)=engine.frontend.mutate(|state|{
  let mut imported=0;let mut skipped=0;
  for p in &prepared{
   let id=format!("dominion-{}",p.slug);
   if state.agents.contains_key(&id)&&!body.refresh_identity{skipped+=1;continue;}
   if !state.agents.contains_key(&id)&&state.agents.len()>=128{
    return Err("maximum 128 agents reached".into());
   }
   let before=state.agents.get(&id).cloned();
   let agent=UiAgent{
    id:id.clone(),name:p.name.clone(),description:p.description.clone(),
    system_prompt:p.prompt.clone(),
    provider_id:before.as_ref().map(|p|p.provider_id.clone())
     .unwrap_or_else(||"dominion-native".into()),
    model:before.as_ref().map(|p|p.model.clone()).unwrap_or_else(||model.clone()),
    color:p.color.clone(),role:p.role.clone(),
    reasoning_effort:before.as_ref().and_then(|a|a.reasoning_effort.clone()),
    thinking:before.as_ref().and_then(|a|a.thinking),
    aliases:before.as_ref().map(|a|a.aliases.clone()).unwrap_or_default(),
    avatar_url:before.as_ref().map(|a|a.avatar_url.clone()).unwrap_or_default(),
    avatar:if p.avatar.is_empty(){before.as_ref().map(|a|a.avatar.clone()).unwrap_or_default()}
     else{p.avatar.clone()},
   };
   state.agents.insert(id,agent);imported+=1;
  }
  Ok((imported,skipped))
 }).await.map_err(|error|(StatusCode::BAD_REQUEST,Json(json!({"error":error}))))?;
 Ok(Json(json!({"imported":imported,"skipped":skipped,"errors":errors})))
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test]
 fn only_bounded_local_soul_files_are_imported(){
  let root=std::env::temp_dir().join(format!("dominion-personas-{}",uuid::Uuid::new_v4()));
  let dir=root.join("ainz");
  std::fs::create_dir_all(&dir).unwrap();
  std::fs::write(dir.join("SOUL.md"),"# 정체성: 아인즈\n담당: 설계\n실제 프로필").unwrap();
  let p=load(&root.canonicalize().unwrap(),"ainz","아인즈","#4B3B73").unwrap();
  assert_eq!(p.role,"설계");
  assert!(p.prompt.contains("실제 프로필"));
  std::fs::write(dir.join("SOUL.md"),vec![b'x';MAX_SOUL+1]).unwrap();
  assert!(load(&root.canonicalize().unwrap(),"ainz","아인즈","#4B3B73").is_err());
  std::fs::remove_dir_all(root).unwrap();
 }
}
