//! One-click, local-first, restartable installation of a complete official
//! ComfyUI video model pack. No shell execution, custom nodes or arbitrary URLs.
//! The 3 authoritative assets come from Comfy-Org's official Wan2.2 workflow.
//! Every downloaded byte is checked against Hugging Face LFS SHA-256 before
//! being atomically moved into the selected ComfyUI model directory.
use crate::{config::Settings,engine::Engine};
use axum::{extract::State,http::StatusCode,routing::{get,post},Json,Router};
use reqwest::{Client,StatusCode as HttpStatus};
use serde::Deserialize;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{fs,io::Read,path::{Path,PathBuf},sync::{
 Arc,OnceLock,atomic::{AtomicBool,Ordering}
},time::Duration};
use tokio::{io::AsyncWriteExt,sync::Mutex};
use uuid::Uuid;

type Fault=(StatusCode,Json<Value>);
fn bad(message:impl Into<String>)->Fault{
 (StatusCode::BAD_REQUEST,Json(json!({"error":message.into()})))
}
#[derive(Clone,Copy)]
struct Asset{folder:&'static str,name:&'static str,repo:&'static str,estimate:u64}
const WAN22:[Asset;3]=[
 Asset{folder:"diffusion_models",name:"wan2.2_ti2v_5B_fp16.safetensors",
  repo:"Comfy-Org/Wan_2.2_ComfyUI_Repackaged",estimate:9_310_000_000},
 Asset{folder:"text_encoders",name:"umt5_xxl_fp8_e4m3fn_scaled.safetensors",
  repo:"Comfy-Org/Wan_2.1_ComfyUI_repackaged",estimate:6_270_000_000},
 Asset{folder:"vae",name:"wan2.2_vae.safetensors",
  repo:"Comfy-Org/Wan_2.2_ComfyUI_Repackaged",estimate:1_310_000_000},
];
const PACK_ID:&str="wan22-ti2v-5b";
const MAX_BYTES:u64=24*1024*1024*1024;
#[derive(Default)]
struct Active{
 state:String,file:String,bytes:u64,total:u64,error:String,
 cancel:Option<Arc<AtomicBool>>
}
static ACTIVE:OnceLock<Mutex<Active>>=OnceLock::new();
fn active()->&'static Mutex<Active>{ACTIVE.get_or_init(||Mutex::new(Active::default()))}
fn root_config(settings:&Settings)->PathBuf{
 settings.data_dir.join("video-models-root.json")
}
fn validated_root(path:&Path)->Result<PathBuf,String>{
 if !path.is_absolute(){return Err("ComfyUI models folder must be an absolute path".into());}
 let actual=fs::canonicalize(path).map_err(|e|format!(
  "ComfyUI models folder does not exist: {e}"))?;
 if !actual.file_name().is_some_and(|name|name.to_string_lossy().eq_ignore_ascii_case("models")){
  return Err("Select your existing ComfyUI/models folder, not its parent".into());
 }
 if !actual.is_dir(){return Err("Selected models path is not a folder".into());}
 let has_model_folders=["checkpoints","vae","diffusion_models","text_encoders"]
  .iter().any(|dir|actual.join(dir).is_dir());
 let parent_is_comfy=actual.parent().is_some_and(|parent|
  parent.join("folder_paths.py").is_file()||
  parent.join("main.py").is_file());
 if !has_model_folders&&!parent_is_comfy{
  return Err("The chosen folder does not look like a ComfyUI/models installation".into());
 }
 Ok(actual)
}
fn remember_root(settings:&Settings,root:&Path)->Result<(),String>{
 let path=root_config(settings);
 let dir=path.parent().ok_or("invalid video model configuration location")?;
 fs::create_dir_all(dir).map_err(|e|e.to_string())?;
 let temp=dir.join(format!(".model-root-{}.tmp",Uuid::new_v4()));
 let bytes=serde_json::to_vec(&json!({"root":root.to_string_lossy()}))
  .map_err(|e|e.to_string())?;
 let outcome=(||->Result<(),String>{
  use std::io::Write;
  let mut options=fs::OpenOptions::new();
  options.write(true).create_new(true);
  #[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
  let mut file=options.open(&temp).map_err(|e|e.to_string())?;
  file.write_all(&bytes).map_err(|e|e.to_string())?;
  file.sync_all().map_err(|e|e.to_string())?;
  drop(file);
  fs::rename(&temp,&path).map_err(|e|e.to_string())?;
  #[cfg(unix)]fs::File::open(dir).and_then(|f|f.sync_all()).map_err(|e|e.to_string())?;
  Ok(())
 })();
 if outcome.is_err(){let _=fs::remove_file(temp);}
 outcome
}
fn detect_root(settings:&Settings)->Option<PathBuf>{
 let saved=root_config(settings);
 if let Ok(bytes)=fs::read(saved){
  if bytes.len()<4096{
   if let Ok(data)=serde_json::from_slice::<Value>(&bytes){
    if let Some(path)=data["root"].as_str(){
     if let Ok(root)=validated_root(Path::new(path)){return Some(root);}
    }
   }
  }
 }
 if let Some(path)=std::env::var_os("DOMINION_COMFY_MODELS_DIR"){
  if let Ok(root)=validated_root(Path::new(&path)){return Some(root);}
 }
 let mut base=Vec::new();
 if let Ok(cwd)=std::env::current_dir(){base.push(cwd);}
 if let Some(home)=std::env::var_os("USERPROFILE").or_else(||std::env::var_os("HOME")){
  let home=PathBuf::from(home);
  base.push(home.clone());base.push(home.join("Downloads"));
 }
 if let Ok(bin)=std::env::current_exe(){
  if let Some(dir)=bin.parent(){base.push(dir.to_path_buf());}
 }
 for base in base{
  for path in [
   base.join("models"),
   base.join("ComfyUI").join("models"),
   base.join("ComfyUI_windows_portable").join("ComfyUI").join("models"),
  ]{
   if let Ok(root)=validated_root(&path){return Some(root);}
  }
 }
 None
}
/// Managed model library: first-run install needs no folder picker or
/// pre-existing ComfyUI directory. Prefer an existing ComfyUI library when
/// auto-discovered so the live renderer can load files immediately.
fn default_root(settings:&Settings)->PathBuf{
 let requested=settings.data_dir.join("video-models").join("models");
 if requested.is_absolute(){return requested;}
 std::env::current_dir().unwrap_or_else(|_|PathBuf::from(".")).join(requested)
}
fn choose_root(settings:&Settings)->Result<PathBuf,String>{
 if let Some(existing)=detect_root(settings){return Ok(existing);}
 let managed=default_root(settings);
 // Keep the managed library outside frontend public static directories.
 // Create subdirectories before accepting it as an installed library.
 for folder in ["diffusion_models","text_encoders","vae"]{
  fs::create_dir_all(managed.join(folder)).map_err(|e|
   format!("Could not prepare the Dominion video model library: {e}"))?;
 }
 validated_root(&managed)
}
fn size(path:&Path)->u64{fs::metadata(path).map(|meta|meta.len()).unwrap_or(0)}
fn target(root:&Path,file:Asset)->PathBuf{root.join(file.folder).join(file.name)}
fn progress(root:&Path)->(u64,bool,Vec<Value>){
 let mut bytes=0;let mut installed=true;
 let files=WAN22.iter().map(|file|{
  let path=target(root,*file);
  let present=size(&path);
  let partial=size(&path.with_file_name(format!("{}.part",file.name)));
  // A complete model file is large; a truncated or HTML error is not installed.
  let ok=present>=file.estimate.saturating_mul(95)/100;
  installed&=ok;
  bytes+=if ok{file.estimate}else{partial.min(file.estimate)};
  json!({"name":file.name,"folder":file.folder,
   "expectedBytes":file.estimate,"installed":ok,
   "downloadedBytes":if ok{present}else{partial}})
 }).collect();
 (bytes,installed,files)
}
pub fn routes()->Router<Arc<Engine>>{
 Router::new()
  .route("/video/model-packs",get(status))
  .route("/video/model-packs/install",post(install))
  .route("/video/model-packs/cancel",post(cancel))
}
pub(crate) async fn status(State(engine):State<Arc<Engine>>)->Json<Value>{
 let discovered=detect_root(&engine.settings);
 let root=discovered.clone().unwrap_or_else(||default_root(&engine.settings));
 let (bytes,installed,files)=progress(&root);
 let state=active().lock().await;
 Json(json!({
  "id":PACK_ID,"name":"Wan 2.2 TI2V 5B",
  "label":"8GB VRAM 오프로딩 영상 생성 모델",
  "origin":"Comfy-Org 공식 ComfyUI 워크플로",
  "estimatedBytes":WAN22.iter().map(|m|m.estimate).sum::<u64>(),
  "modelsDir":root.to_string_lossy().to_string(),
  "managedLibrary":discovered.is_none()&&root==default_root(&engine.settings),
  "installed":installed,"configured":crate::frontend_comfy::config(&engine.settings)
   .ok().is_some_and(|cfg|cfg.workflow_name=="Dominion · Wan 2.2 TI2V 5B"),
  "state":if state.state.is_empty(){if installed{"installed"}else{"idle"}}else{&state.state},
  "currentFile":state.file,"error":state.error,
  "downloadedBytes":if state.state=="downloading"{state.bytes.max(bytes)}else{bytes},
  "totalBytes":if state.total>0{state.total}else{WAN22.iter().map(|m|m.estimate).sum::<u64>()},
  "files":files,
  "needsComfyRestart":installed
 }))
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct InstallRequest{#[serde(default)]models_dir:Option<String>}
async fn install(State(engine):State<Arc<Engine>>,Json(request):Json<InstallRequest>)
 ->Result<(StatusCode,Json<Value>),Fault>{
 let root=if let Some(raw)=request.models_dir.as_deref().filter(|s|!s.trim().is_empty()){
  validated_root(Path::new(raw)).map_err(bad)?
 }else{
  choose_root(&engine.settings).map_err(bad)?
 };
 let cancelled=Arc::new(AtomicBool::new(false));
 {
  let mut active=active().lock().await;
  if active.state=="downloading"{return Err((
   StatusCode::CONFLICT,Json(json!({"error":"영상 모델 다운로드가 진행 중입니다."}))));
  }
  active.state="downloading".into();active.file="설치 준비".into();
  active.error.clear();active.bytes=0;active.total=0;
  active.cancel=Some(Arc::clone(&cancelled));
 }
 if let Err(error)=remember_root(&engine.settings,&root){
  let mut active=active().lock().await;
  active.state="failed".into();active.error=error.clone();active.cancel=None;
  return Err(bad(error));
 }
 let settings=engine.settings.clone();
 let worker_root=root.clone();
 tokio::spawn(async move{
  let outcome=install_assets(&settings,&worker_root,Arc::clone(&cancelled)).await;
  let mut state=active().lock().await;
  match outcome{
   Ok(())=>{state.state="installed".into();state.file="설치 완료".into();state.error.clear();},
   Err(error) if cancelled.load(Ordering::SeqCst)=>{
    state.state="paused".into();state.error="다운로드를 중단했습니다. 다시 설치를 누르면 이어받습니다.".into();
   },
   Err(error)=>{state.state="failed".into();state.error=error;},
  }
  state.cancel=None;
 });
 Ok((StatusCode::ACCEPTED,Json(json!({"ok":true,"model":PACK_ID,
  "modelsDir":root.to_string_lossy(),"state":"downloading"}))))
}
async fn cancel()->Json<Value>{
 let active=active().lock().await;
 if let Some(flag)=&active.cancel{flag.store(true,Ordering::SeqCst);}
 Json(json!({"ok":true,"state":active.state}))
}
fn client()->Result<Client,String>{
 Client::builder()
  // Weight files can take hours. A request-wide timeout would abort a 9GB
  // transfer even while bytes keep flowing. Limit CONNECT and per-chunk idle,
  // not the entire response lifetime.
  .connect_timeout(Duration::from_secs(25))
  .redirect(reqwest::redirect::Policy::custom(|attempt|{
   let host=attempt.url().host_str().unwrap_or("");
   if attempt.previous().len()<6&&attempt.url().scheme()=="https"&&(
    host=="huggingface.co"||host.ends_with(".hf.co")||host.ends_with(".xethub.hf.co")
   ){attempt.follow()}else{attempt.stop()}
  }))
  .build().map_err(|e|e.to_string())
}
async fn metadata(client:&Client,asset:Asset)->Result<(u64,String),String>{
 let url=format!("https://huggingface.co/api/models/{}/tree/main/split_files/{}",
  asset.repo,asset.folder);
 let response=client.get(url).timeout(Duration::from_secs(35)).send().await
  .map_err(|e|format!("Hugging Face metadata: {e}"))?;
 if !response.status().is_success(){return Err(format!("Hugging Face metadata HTTP {}",response.status()));}
 if response.content_length().is_some_and(|n|n>512*1024){
  return Err("Hugging Face metadata too large".into());
 }
 let payload=tokio::time::timeout(Duration::from_secs(35),response.bytes())
  .await.map_err(|_|"Hugging Face metadata response timeout")?
  .map_err(|e|e.to_string())?;
 if payload.len()>512*1024{return Err("Hugging Face metadata too large".into());}
 let records:Value=serde_json::from_slice(&payload).map_err(|e|e.to_string())?;
 let path=format!("split_files/{}/{}",asset.folder,asset.name);
 let record=records.as_array().and_then(|list|list.iter().find(|row|
  row["path"].as_str()==Some(&path))).ok_or("Official model file not listed by Hugging Face")?;
 let size=record["size"].as_u64().or_else(||record["lfs"]["size"].as_u64())
  .ok_or("Official model file lacks declared size")?;
 let sha=record["lfs"]["oid"].as_str().or_else(||record["lfs"]["sha256"].as_str())
  .ok_or("Official model file lacks SHA-256")?.to_lowercase();
 if !(1024..=MAX_BYTES).contains(&size)||sha.len()!=64||
   !sha.bytes().all(|ch|ch.is_ascii_hexdigit()){
  return Err("Hugging Face returned invalid LFS size or checksum".into());
 }
 Ok((size,sha))
}
async fn sha256(path:PathBuf)->Result<String,String>{
 tokio::task::spawn_blocking(move||{
  let mut file=fs::File::open(path).map_err(|e|e.to_string())?;
  let mut hash=Sha256::new();
  let mut buffer=vec![0u8;2*1024*1024];
  loop{
   let count=file.read(&mut buffer).map_err(|e|e.to_string())?;
   if count==0{break;}
   hash.update(&buffer[..count]);
  }
  Ok(format!("{:x}",hash.finalize()))
 }).await.map_err(|e|e.to_string())?
}
fn verified_directory(root:&Path,folder:&str)->Result<PathBuf,String>{
 let directory=root.join(folder);
 fs::create_dir_all(&directory).map_err(|e|e.to_string())?;
 let canonical=fs::canonicalize(directory).map_err(|e|e.to_string())?;
 if !canonical.starts_with(root){
  return Err("ComfyUI model folder escapes the selected installation".into());
 }
 Ok(canonical)
}
async fn install_assets(settings:&Settings,root:&Path,cancelled:Arc<AtomicBool>)
 ->Result<(),String>{
 let client=client()?;
 let mut completed=0u64;
 for asset in WAN22{
  if cancelled.load(Ordering::SeqCst){return Err("cancelled".into());}
  let folder=verified_directory(root,asset.folder)?;
  let destination=folder.join(asset.name);
  let part=folder.join(format!("{}.part",asset.name));
  // Refuse symlinked output files: no attacker-controlled redirection of
  // multi-gigabyte writes outside the selected ComfyUI models root.
  for path in [&destination,&part]{
   if fs::symlink_metadata(path).is_ok_and(|m|m.file_type().is_symlink()){
    return Err(format!("Refusing symlinked model path: {}",path.display()));
   }
  }
  let (length,expected_hash)=metadata(&client,asset).await?;
  {
   let mut state=active().lock().await;
   state.file=format!("{}/{}",asset.folder,asset.name);
   state.total=WAN22.iter().map(|v|v.estimate).sum();
   state.bytes=completed;
  }
  if size(&destination)==length{
   if sha256(destination.clone()).await?==expected_hash{
    completed=completed.saturating_add(length);
    continue;
   }
   return Err(format!("Existing {} has a checksum mismatch. Move it aside and retry.",asset.name));
  }else if destination.exists(){
   return Err(format!("Existing {} has the wrong size; it will not be overwritten.",asset.name));
  }
  let mut offset=size(&part);
  if offset>length{
   tokio::fs::remove_file(&part).await.map_err(|e|e.to_string())?;
   offset=0;
  }
  if offset<length{
   let url=format!("https://huggingface.co/{}/resolve/main/split_files/{}/{}",
    asset.repo,asset.folder,asset.name);
   let mut request=client.get(url);
   if offset>0{request=request.header(reqwest::header::RANGE,format!("bytes={offset}-"));}
   let mut response=tokio::time::timeout(Duration::from_secs(60),request.send())
    .await.map_err(|_|"Official model connection timed out")?
    .map_err(|e|format!("Official model download: {e}"))?;
   let append=if offset>0&&response.status()==HttpStatus::PARTIAL_CONTENT{
    let expected=format!("bytes {offset}-");
    let range=response.headers().get(reqwest::header::CONTENT_RANGE)
     .and_then(|v|v.to_str().ok()).unwrap_or("");
    if !range.starts_with(&expected){return Err("Resume response returned incorrect content range".into());}
    true
   }else if response.status()==HttpStatus::OK{
    offset=0;false
   }else{return Err(format!("Official model download HTTP {}",response.status()));};
   let mut options=tokio::fs::OpenOptions::new();
   options.write(true).create(true);
   if append{options.append(true);}else{options.truncate(true);}
   let mut output=options.open(&part).await.map_err(|e|e.to_string())?;
   while let Some(chunk)=tokio::time::timeout(Duration::from_secs(120),response.chunk())
    .await.map_err(|_|"Model transfer stalled; select install again to resume")?
    .map_err(|e|e.to_string())?{
    if cancelled.load(Ordering::SeqCst){
     output.sync_all().await.map_err(|e|e.to_string())?;
     return Err("cancelled".into());
    }
    offset=offset.saturating_add(chunk.len() as u64);
    if offset>length{return Err("Download exceeds declared Hugging Face LFS size".into());}
    output.write_all(&chunk).await.map_err(|e|e.to_string())?;
    let mut state=active().lock().await;
    state.bytes=completed.saturating_add(offset);
   }
   output.sync_all().await.map_err(|e|e.to_string())?;
  }
  if size(&part)!=length{return Err(format!("Partial download for {}: select install again to resume",asset.name));}
  {
   let mut state=active().lock().await;
   state.file=format!("검증 중: {}",asset.name);
  }
  if sha256(part.clone()).await?!=expected_hash{
   tokio::fs::remove_file(&part).await.map_err(|e|e.to_string())?;
   return Err(format!("SHA-256 mismatch for {}; discarded invalid download",asset.name));
  }
  tokio::fs::rename(&part,&destination).await.map_err(|e|e.to_string())?;
  completed=completed.saturating_add(length);
 }
 // Official template is checked into Dominion and contains only ComfyUI's
 // built-in nodes. No arbitrary code or download-time workflow is executed.
 let workflow:Value=serde_json::from_str(include_str!("wan22_5b_api.json"))
  .map_err(|e|e.to_string())?;
 crate::frontend_comfy::update(settings,&json!({
  "engine":"comfyui",
  "comfyWorkflow":workflow,
  "comfyWorkflowName":"Dominion · Wan 2.2 TI2V 5B",
  "videoModel":"diffusion_models/wan2.2_ti2v_5B_fp16.safetensors",
  "resolution":"480x832",
  "steps":20
 }))?;
 Ok(())
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test]
 fn official_model_pack_and_api_workflow_are_closed_and_consistent(){
  assert_eq!(WAN22.len(),3);
  assert!(WAN22.iter().all(|a|!a.name.contains('/')&&!a.folder.contains('/')));
  assert!(WAN22.iter().all(|a|a.repo.starts_with("Comfy-Org/")));
  let graph:Value=serde_json::from_str(include_str!("wan22_5b_api.json")).unwrap();
  assert_eq!(graph["37"]["inputs"]["unet_name"],WAN22[0].name);
  assert_eq!(graph["38"]["inputs"]["clip_name"],WAN22[1].name);
  assert_eq!(graph["39"]["inputs"]["vae_name"],WAN22[2].name);
  assert!(graph.as_object().unwrap().values().all(|v|
   v["class_type"]!="LoadImage"&&v["class_type"]!="ExecutePython"));
  let dir=std::env::temp_dir().join(format!("wan22-pack-{}",Uuid::new_v4()));
  let mut settings=Settings::from_env().unwrap();
  settings.data_dir=dir.clone();
  let cfg=crate::frontend_comfy::update(&settings,&json!({
   "engine":"comfyui","comfyWorkflow":graph,
   "comfyWorkflowName":"Dominion · Wan 2.2 TI2V 5B",
   "videoModel":"diffusion_models/wan2.2_ti2v_5B_fp16.safetensors"
  })).unwrap();
  assert_eq!(cfg.engine,"comfyui");
  assert!(cfg.comfy_workflow.is_some());
  fs::remove_dir_all(dir).unwrap();
 }
 #[test]
 fn creates_own_model_library_without_manual_paths_or_comfyui(){
  let mut settings=Settings::from_env().unwrap();
  let data=std::env::temp_dir().join(format!("dominion-video-{}",Uuid::new_v4()));
  settings.data_dir=data.clone();
  let managed=choose_root(&settings).unwrap();
  assert_eq!(managed,fs::canonicalize(data.join("video-models/models")).unwrap());
  assert!(managed.join("vae").is_dir());
  assert!(managed.join("text_encoders").is_dir());
  assert!(managed.join("diffusion_models").is_dir());
  assert_eq!(progress(&managed).1,false);
  fs::remove_dir_all(data).unwrap();
 }
 #[test]
 fn rejects_paths_without_existing_comfyui_model_directory(){
  assert!(validated_root(Path::new("models")).is_err());
  let dir=std::env::temp_dir().join(format!("video-root-{}",Uuid::new_v4()));
  fs::create_dir_all(dir.join("ComfyUI/models/vae")).unwrap();
  assert_eq!(validated_root(&dir.join("ComfyUI/models")).unwrap(),
   fs::canonicalize(dir.join("ComfyUI/models")).unwrap());
  fs::remove_dir_all(dir).unwrap();
 }
}
