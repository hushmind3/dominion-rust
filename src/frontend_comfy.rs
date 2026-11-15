//! Local ComfyUI integration for the preserved main Video page.
//! Accepts API-format workflow graphs, sends them to ComfyUI's real /prompt
//! queue, follows /history/{id}, and retains verified generated video bytes.
//! Never executes user-uploaded scripts or imports arbitrary Python nodes.
use crate::{config::Settings,domain::now_ms};
use reqwest::{Client,Url};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::{collections::HashMap,fs,io::Write,path::{Path,PathBuf},
 sync::{Mutex,OnceLock},time::Duration};
use tokio::io::{AsyncReadExt,AsyncWriteExt};
use sha2::Digest;
use uuid::Uuid;

const MAX_GRAPH:usize=1024*1024;
const MAX_HISTORY:u64=8*1024*1024;
const MAX_VIDEO:u64=2*1024*1024*1024;
static LOCK:OnceLock<Mutex<()>>=OnceLock::new();
fn guard()->std::sync::MutexGuard<'static,()>{
 LOCK.get_or_init(||Mutex::new(())).lock().unwrap_or_else(|e|e.into_inner())
}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Config {
 pub engine:String,
 pub comfy_url:String,
 pub workflow_name:String,
 #[serde(default)]pub comfy_workflow:Option<Value>,
 pub resolution:String,
 pub steps:u32,
 pub video_model:String,
 pub voice:String,
 pub narration:bool
}
impl Default for Config {
 fn default()->Self{
  Self{engine:"native".into(),comfy_url:"http://127.0.0.1:8188".into(),
   workflow_name:String::new(),comfy_workflow:None,
   resolution:"480x832".into(),steps:20,video_model:String::new(),
   voice:"Yuna".into(),narration:true}
 }
}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Job {
 pub id:Uuid,pub prompt_id:Option<Uuid>,
 pub title:String,pub brief:String,pub aspect_ratio:String,pub duration:u32,
 pub status:String,pub error:Option<String>,pub format:Option<String>,
 pub created_at_ms:u64,pub updated_at_ms:u64,
 #[serde(default)]pub bytes:u64,
}
#[derive(Default,Serialize,Deserialize)]
struct Journal { jobs:HashMap<Uuid,Job> }
fn root(settings:&Settings)->PathBuf{settings.data_dir.join("comfy-runtime")}
fn config_path(settings:&Settings)->PathBuf{root(settings).join("config.json")}
fn journal_path(settings:&Settings)->PathBuf{root(settings).join("jobs.json")}
fn atomic(path:&Path,bytes:&[u8])->Result<(),String>{
 let dir=path.parent().ok_or("ComfyUI state parent missing")?;
 fs::create_dir_all(dir).map_err(|e|e.to_string())?;
 let temp=dir.join(format!(".state-{}.tmp",Uuid::new_v4()));
 let result=(||->Result<(),String>{
  let mut opts=fs::OpenOptions::new();opts.create_new(true).write(true);
  #[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;opts.mode(0o600);}
  let mut out=opts.open(&temp).map_err(|e|e.to_string())?;
  out.write_all(bytes).map_err(|e|e.to_string())?;
  out.sync_all().map_err(|e|e.to_string())?;
  drop(out);
  fs::rename(&temp,path).map_err(|e|e.to_string())?;
  #[cfg(unix)]fs::File::open(dir).and_then(|f|f.sync_all()).map_err(|e|e.to_string())?;
  Ok(())
 })();
 if result.is_err(){let _=fs::remove_file(temp);}
 result
}
fn read_json<T:serde::de::DeserializeOwned+Default>(path:&Path)->Result<T,String>{
 if !path.exists(){return Ok(T::default());}
 #[cfg(unix)]{
  use std::os::unix::fs::PermissionsExt;
  if fs::metadata(path).map_err(|e|e.to_string())?.permissions().mode()&0o077!=0{
   return Err("ComfyUI state permissions must be owner-only".into());
  }
 }
 let metadata=fs::metadata(path).map_err(|e|e.to_string())?;
 if metadata.len()>8*1024*1024{return Err("ComfyUI state too large".into());}
 serde_json::from_slice(&fs::read(path).map_err(|e|e.to_string())?)
  .map_err(|e|format!("ComfyUI state corrupted: {e}"))
}
pub fn config(settings:&Settings)->Result<Config,String>{
 read_json(&config_path(settings))
}
fn valid_url(raw:&str)->Result<String,String>{
 let url=Url::parse(raw.trim()).map_err(|e|format!("ComfyUI URL: {e}"))?;
 let loopback=url.host_str().is_some_and(|host|
  host=="localhost"||host.parse::<std::net::IpAddr>().is_ok_and(|ip|ip.is_loopback()));
 if url.scheme()!="http"||!loopback||!url.username().is_empty()||url.password().is_some()
  ||url.fragment().is_some()||url.query().is_some()
  ||url.path()!="/"{return Err("ComfyUI URL must be a bare HTTP loopback origin".into());}
 Ok(url.origin().ascii_serialization())
}
fn valid_graph(value:&Value)->Result<(),String>{
 let bytes=serde_json::to_vec(value).map_err(|e|e.to_string())?;
 if bytes.len()>MAX_GRAPH{return Err("ComfyUI API graph exceeds 1 MiB".into());}
 let nodes=value.as_object().ok_or("ComfyUI workflow must be a node-ID map exported in API format")?;
 if nodes.is_empty()||nodes.len()>256{return Err("ComfyUI API graph requires 1-256 nodes".into());}
 let mut has_prompt=false;
 let mut has_video=false;
 let mut input_slot=false;
 for (id,node) in nodes{
  if id.is_empty()||id.len()>64||!id.bytes().all(|b|b.is_ascii_digit()){
   return Err("ComfyUI API graph contains invalid node IDs".into());
  }
  let class=node["class_type"].as_str().ok_or("ComfyUI node has no class_type")?;
  if class.is_empty()||class.len()>120{return Err("invalid ComfyUI node class".into());}
  let input=node["inputs"].as_object().ok_or("ComfyUI node has no inputs")?;
  if matches!(class,"VHS_VideoCombine"|"SaveVideo"|"SaveWEBM"|"SaveMP4"){has_video=true;}
  for (key,value) in input{
   if key.len()>120{return Err("ComfyUI input name too long".into());}
   if let Some(string)=value.as_str(){
    if string.contains("{{PROMPT}}"){has_prompt=true;}
    if (key=="text"||key=="prompt")&&
       (class.contains("TextEncode")||class.contains("TextEncoder")){
     input_slot=true;
    }
   }
  }
 }
 if !has_video{return Err("Workflow needs a video output node (SaveVideo or VHS_VideoCombine)".into());}
 if !has_prompt&&!input_slot{
  return Err("Workflow needs {{PROMPT}} or a text encoder's text/prompt input".into());
 }
 Ok(())
}
fn workflow_id(cfg:&Config)->Option<String>{
 cfg.comfy_workflow.as_ref().map(|graph|{
  let hash=sha2::Sha256::digest(serde_json::to_vec(graph).unwrap_or_default());
  format!("workflow-{:x}",hash)[..24].to_string()
 })
}
pub fn workflows(cfg:&Config)->Vec<Value>{
 workflow_id(cfg).map(|id|vec![json!({
  "id":id,"name":cfg.workflow_name,"engine":"comfyui",
  "source":"user_api_graph"
 })]).unwrap_or_default()
}
pub fn public_config(cfg:&Config)->Value{
 json!({"engine":cfg.engine,"comfyUrl":cfg.comfy_url,
  "voice":cfg.voice,"narration":cfg.narration,
  "videoModel":cfg.video_model,"resolution":cfg.resolution,"steps":cfg.steps})
}
pub fn update(settings:&Settings,patch:&Value)->Result<Config,String>{
 let body=patch.as_object().ok_or("video config patch must be JSON object")?;
 let _held=guard();
 let mut cfg=config(settings)?;
 for (name,value) in body{
  match name.as_str(){
   "engine"=>{
    let engine=value.as_str().ok_or("engine must be text")?;
    if !matches!(engine,"native"|"comfyui"){return Err("unsupported video engine".into());}
    cfg.engine=engine.into();
   },
   "comfyUrl"=>{
    let uri=value.as_str().ok_or("comfyUrl must be text")?;
    if !uri.trim().is_empty(){cfg.comfy_url=valid_url(uri)?;}
   },
   "comfyWorkflow"=>{
    valid_graph(value)?;
    cfg.comfy_workflow=Some(value.clone());
    cfg.engine="comfyui".into();
   },
   "comfyWorkflowName"=>{
    let name=value.as_str().ok_or("comfyWorkflowName must be text")?;
    if name.trim().is_empty()||name.len()>180{return Err("invalid ComfyUI workflow name".into());}
    cfg.workflow_name=name.into();
   },
   "voice"=>{
    if value.as_str()!=Some("Yuna"){return Err("only installed Yuna voice preset is supported".into());}
    cfg.voice="Yuna".into();
   },
   "narration"=>cfg.narration=value.as_bool().ok_or("narration must be boolean")?,
   "videoModel"=>{
    let model=value.as_str().ok_or("videoModel must be text")?;
    if model.len()>240{return Err("videoModel name too long".into());}
    cfg.video_model=model.to_owned();
   },
   "resolution"=>{
    let resolution=value.as_str().ok_or("resolution must be text")?;
    if !matches!(resolution,"256x448"|"384x672"|"480x832"|"832x480"|"448x448"){
     return Err("unsupported video resolution".into());
    }
    cfg.resolution=resolution.into();
   },
   "steps"=>{
    let steps=value.as_u64().ok_or("steps must be integer")?;
    if !(1..=100).contains(&steps){return Err("steps outside 1..100".into());}
    cfg.steps=steps as u32;
   },
   _=>return Err(format!("unknown video config setting: {name}"))
  }
 }
 if cfg.comfy_workflow.is_some()&&cfg.workflow_name.is_empty(){
  cfg.workflow_name="Imported ComfyUI API Workflow".into();
 }
 atomic(&config_path(settings),&serde_json::to_vec(&cfg).map_err(|e|e.to_string())?)?;
 Ok(cfg)
}
fn client()->Result<Client,String>{
 Client::builder().timeout(Duration::from_secs(20))
  .redirect(reqwest::redirect::Policy::none())
  .build().map_err(|e|e.to_string())
}
async fn json_response(response:reqwest::Response,max:u64)->Result<Value,String>{
 if !response.status().is_success(){return Err(format!("ComfyUI HTTP {}",response.status()));}
 if response.content_length().is_some_and(|size|size>max){
  return Err("ComfyUI response exceeds size limit".into());
 }
 let bytes=response.bytes().await.map_err(|e|e.to_string())?;
 if bytes.len() as u64>max{return Err("ComfyUI response exceeds size limit".into());}
 serde_json::from_slice(&bytes).map_err(|e|format!("ComfyUI JSON invalid: {e}"))
}
/// Query the installed model files from the running ComfyUI server rather
/// than advertising a hard-coded model catalog. A selected model must belong
/// to a concrete loader node before it can influence generation.
pub async fn catalog(settings:&Settings)->Vec<Value>{
 let Ok(cfg)=config(settings)else{return Vec::new()};
 let Ok(base)=valid_url(&cfg.comfy_url)else{return Vec::new()};
 let Ok(http)=client()else{return Vec::new()};
 let mut items=Vec::new();
 for folder in ["diffusion_models","checkpoints"]{
  let result=http.get(format!("{base}/models/{folder}"))
   .timeout(Duration::from_secs(4)).send().await;
  let Ok(response)=result else{continue};
  let Ok(data)=json_response(response,MAX_HISTORY).await else{continue};
  let Some(installed)=data.as_array()else{continue};
  for model in installed.iter().take(200){
   let Some(name)=model.as_str()else{continue};
   if name.is_empty()||name.len()>220||name.contains("..")||
    name.chars().any(char::is_control){continue;}
   items.push(json!({"id":format!("{folder}/{name}"),"name":name,
    "kind":folder,"available":true,
    "hint":"ComfyUI reported this file installed; selected workflow must use a compatible model loader"}));
  }
 }
 items
}
pub async fn reachable(cfg:&Config)->bool{
 let Ok(url)=valid_url(&cfg.comfy_url)else{return false;};
 let Ok(http)=client() else{return false;};
 match http.get(format!("{url}/system_stats"))
  .timeout(Duration::from_secs(3)).send().await{
  Ok(result)=>result.status().is_success(),
  Err(_)=>false
 }
}
#[derive(Clone)]
pub struct Create {
 pub title:String,pub brief:String,pub negative:String,
 pub aspect_ratio:String,pub duration:u32,pub fps:u32,pub seed:i64,
}
fn dimensions(cfg:&Config,aspect:&str)->(u32,u32){
 let (a,b)=cfg.resolution.split_once('x').and_then(|(a,b)|
  Some((a.parse::<u32>().ok()?,b.parse::<u32>().ok()?)))
  .unwrap_or((480,832));
 match aspect{
  "16:9"=>(a.max(b),a.min(b)),
  "1:1"=>(448,448),
  _=>(a.min(b),a.max(b))
 }
}
fn fill_template(workflow:&Value,params:&Create,cfg:&Config)->Result<Value,String>{
 let mut graph=workflow.clone();
 let (width,height)=dimensions(cfg,&params.aspect_ratio);
 let frames=params.duration.checked_mul(params.fps).ok_or("frame count overflow")?;
 let mut replaced_prompt=false;
 let mut model_applied=cfg.video_model.is_empty();
 let selected=cfg.video_model.split_once('/');
 // Workflow input schemas remain intact. Only text templates or literal
 // numeric controls are modified; graph edges (arrays) are never rewritten.
 let mut keys=graph.as_object().ok_or("workflow not a graph")?
  .keys().cloned().collect::<Vec<_>>();
 keys.sort_by_key(|s|s.parse::<u64>().unwrap_or(u64::MAX));
 for id in keys{
  let node=graph.get_mut(&id).ok_or("missing graph node")?;
  let class=node["class_type"].as_str().unwrap_or("").to_owned();
  let title=node.pointer("/_meta/title").and_then(Value::as_str).unwrap_or("").to_lowercase();
  let is_negative=title.contains("negative")||title.contains("네거티브");
  let inputs=node.get_mut("inputs").and_then(Value::as_object_mut).ok_or("invalid workflow inputs")?;
  for (name,value) in inputs.iter_mut(){
   if let (Some((folder,filename)),Some(_))=(selected,value.as_str()){
    let bind=match folder{
     "checkpoints"=>name=="ckpt_name"&&class.contains("CheckpointLoader"),
     "diffusion_models"=>name=="unet_name"&&
      (class.contains("UNETLoader")||class.contains("DiffusionModelLoader")),
     _=>false
    };
    if bind{
     *value=json!(filename);
     model_applied=true;
     continue;
    }
   }
   if let Some(text)=value.as_str(){
    if text.contains("{{PROMPT}}")||text.contains("{{NEGATIVE_PROMPT}}"){
     let next=text.replace("{{PROMPT}}",&params.brief)
      .replace("{{NEGATIVE_PROMPT}}",&params.negative);
     if text.contains("{{PROMPT}}"){replaced_prompt=true;}
     *value=json!(next);
    }else if (name=="text"||name=="prompt")&&
      (class.contains("TextEncode")||class.contains("TextEncoder")) &&
       !replaced_prompt && !is_negative {
      *value=json!(params.brief);
      replaced_prompt=true;
    }else if (name=="text"||name=="prompt")&&is_negative&&
      (class.contains("TextEncode")||class.contains("TextEncoder")){
      *value=json!(params.negative);
    }
   }else if value.is_number(){
    let mapped=match name.as_str(){
     "seed"|"noise_seed"=>Some(json!(params.seed)),
     "steps"=>Some(json!(cfg.steps)),
     "frame_rate"|"fps"=>Some(json!(params.fps)),
     "num_frames"|"frames"|"length"=>Some(json!(frames)),
     "width"=>Some(json!(width)),
     "height"=>Some(json!(height)),
     _=>None
    };
    if let Some(replacement)=mapped{*value=replacement;}
   }
  }
 }
 if !replaced_prompt{return Err("No usable prompt input found in workflow".into());}
 if !model_applied{
  return Err("selected ComfyUI model is not bound to a supported checkpoint/diffusion loader in this workflow".into());
 }
 Ok(graph)
}
fn read_jobs(settings:&Settings)->Result<Journal,String>{read_json(&journal_path(settings))}
fn write_jobs(settings:&Settings,data:&Journal)->Result<(),String>{
 if data.jobs.len()>512{return Err("ComfyUI journal reached 512 jobs".into());}
 atomic(&journal_path(settings),&serde_json::to_vec(data).map_err(|e|e.to_string())?)
}
fn change(settings:&Settings,id:Uuid,apply:impl FnOnce(&mut Job))->Result<(),String>{
 let _held=guard();let mut state=read_jobs(settings)?;
 let job=state.jobs.get_mut(&id).ok_or("ComfyUI job was not found")?;
 apply(job);job.updated_at_ms=now_ms();
 write_jobs(settings,&state)
}
pub fn get(settings:&Settings,id:Uuid)->Result<Option<Job>,String>{
 Ok(read_jobs(settings)?.jobs.remove(&id))
}
pub fn list(settings:&Settings)->Result<Vec<Value>,String>{
 let mut records=read_jobs(settings)?.jobs.into_values().collect::<Vec<_>>();
 records.sort_by_key(|j|std::cmp::Reverse(j.created_at_ms));
 Ok(records.iter().map(public).collect())
}
pub fn public(job:&Job)->Value{
 json!({"id":job.id,"kind":"video","engine":"comfyui",
  "status":job.status,"title":job.title,"brief":job.brief,
  "createdAt":crate::frontend_state::timestamp(job.created_at_ms),
  "updatedAt":crate::frontend_state::timestamp(job.updated_at_ms),
  "progress":if job.status=="completed"{100}else{0},
  "outputUrl":if job.status=="completed"{Some(format!("/api/video/jobs/{}/content",job.id))}else{None},
  "result":if job.status=="completed"{Some(json!({"bytes":job.bytes}))}else{None},
  "error":job.error,"duration":job.duration,"aspectRatio":job.aspect_ratio,
  "costKrw":0,"pricing":"unknown"})
}
pub async fn create(settings:Settings,input:Create)->Result<Value,String>{
 if input.title.trim().is_empty()||input.title.len()>180
  ||input.brief.trim().is_empty()||input.brief.len()>10000
  ||input.negative.len()>4000
  ||!matches!(input.aspect_ratio.as_str(),"9:16"|"16:9"|"1:1")
  ||!(3..=9).contains(&input.duration)||!(1..=60).contains(&input.fps)
  ||input.seed< -1{
  return Err("invalid ComfyUI video request parameters".into());
 }
 let cfg=config(&settings)?;
 if cfg.engine!="comfyui"{return Err("ComfyUI must be selected in video settings".into());}
 valid_url(&cfg.comfy_url)?;
 let workflow=cfg.comfy_workflow.as_ref().ok_or("Import a ComfyUI API workflow first")?;
 valid_graph(workflow)?;
 if !cfg.video_model.is_empty(){
  let installed=catalog(&settings).await;
  if !installed.iter().any(|entry|entry["id"]==cfg.video_model){
   return Err("selected ComfyUI model is not installed or cannot be queried".into());
  }
 }
 let prepared=fill_template(workflow,&input,&cfg)?;
 if !reachable(&cfg).await{return Err("ComfyUI server is not responding".into());}
 let now=now_ms();
 let job=Job{id:Uuid::new_v4(),prompt_id:None,title:input.title.clone(),
  brief:input.brief.clone(),aspect_ratio:input.aspect_ratio.clone(),
  duration:input.duration,status:"queued".into(),error:None,
  format:None,created_at_ms:now,updated_at_ms:now,bytes:0};
 {
  let _held=guard();let mut state=read_jobs(&settings)?;
  if state.jobs.len()>=512{return Err("ComfyUI job history is full".into());}
  state.jobs.insert(job.id,job.clone());write_jobs(&settings,&state)?;
 }
 let result=public(&job);
 tokio::spawn(async move{
  let outcome=execute(&settings,&cfg,job.id,prepared).await;
  if let Err(reason)=outcome{
   if let Err(error)=change(&settings,job.id,|record|{
    record.status="failed".into();record.error=Some(reason);
   }){tracing::error!(%error,"unable to persist failed ComfyUI job");}
  }
 });
 Ok(result)
}
async fn execute(settings:&Settings,cfg:&Config,id:Uuid,prompt:Value)->Result<(),String>{
 let base=valid_url(&cfg.comfy_url)?;
 let http=client()?;
 // Journal records the job *before* queue submission, so a crash cannot
 // accidentally replay a potentially expensive remote generation.
 let sent=http.post(format!("{base}/prompt")).json(&json!({
  "prompt":prompt,"client_id":id.to_string()
 })).send().await.map_err(|e|format!("ComfyUI prompt submission: {e}"))?;
 let receipt=json_response(sent,MAX_HISTORY).await?;
 let remote=receipt["prompt_id"].as_str().ok_or_else(||format!(
  "ComfyUI rejected workflow: {}",
  receipt.get("error").map(|s|s.to_string()).unwrap_or("missing prompt_id".into())))?;
 let prompt_id=Uuid::parse_str(remote).map_err(|_|"ComfyUI returned invalid prompt_id")?;
 change(settings,id,|record|{
  record.status="running".into();
  record.prompt_id=Some(prompt_id);
 })?;
 for _ in 0..900{
  tokio::time::sleep(Duration::from_secs(2)).await;
  let response=http.get(format!("{base}/history/{prompt_id}"))
   .timeout(Duration::from_secs(15)).send().await
   .map_err(|e|format!("ComfyUI history request: {e}"))?;
  let history=json_response(response,MAX_HISTORY).await?;
  let entry=&history[remote];
  if entry.is_null(){continue;}
  if let Some(file)=find_video(entry){
   let (bytes,format)=download(settings,id,cfg,&http,&file).await?;
   change(settings,id,|job|{
    job.status="completed".into();job.bytes=bytes;job.format=Some(format);
    job.error=None;
   })?;
   return Ok(());
  }
  if entry["status"]["status_str"]=="error"||
     entry["status"]["completed"]==true{
   return Err("ComfyUI finished without an MP4/WebM output; export a video-producing API workflow".into());
  }
 }
 Err("ComfyUI generation exceeded 30-minute polling deadline".into())
}
#[derive(Debug)]
struct ResultFile{filename:String,subfolder:String,format:String}
fn find_video(history:&Value)->Option<ResultFile>{
 let outputs=history["outputs"].as_object()?;
 for node in outputs.values(){
  for key in ["videos","gifs","files"]{
   if let Some(items)=node[key].as_array(){
    for item in items{
     let Some(filename)=item["filename"].as_str()else{continue};
     let format=if filename.to_ascii_lowercase().ends_with(".mp4"){"mp4"}
      else if filename.to_ascii_lowercase().ends_with(".webm"){"webm"}else{continue};
     if filename.is_empty()||filename.len()>255||
       filename.contains('/')||filename.contains('\\')||
       filename.contains("..")||filename.chars().any(char::is_control){
      continue;
     }
     let subfolder=item["subfolder"].as_str().unwrap_or("");
     if !subfolder.is_empty()&&subfolder.split('/').any(|part|
        part.is_empty()||part=="."||part==".."||
        !part.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'_'||b==b' ')){
      continue;
     }
     if item["type"].as_str()!=Some("output"){continue;}
     return Some(ResultFile{filename:filename.into(),
      subfolder:subfolder.into(),format:format.into()});
    }
   }
  }
 }
 None
}
async fn download(settings:&Settings,id:Uuid,cfg:&Config,http:&Client,file:&ResultFile)
 ->Result<(u64,String),String>{
 let base=valid_url(&cfg.comfy_url)?;
 let mut url=Url::parse(&format!("{base}/view")).map_err(|e|e.to_string())?;
 url.query_pairs_mut().append_pair("filename",&file.filename)
  .append_pair("subfolder",&file.subfolder).append_pair("type","output");
 let response=http.get(url).timeout(Duration::from_secs(300)).send().await
  .map_err(|e|format!("ComfyUI video download: {e}"))?;
 if !response.status().is_success(){
  return Err(format!("ComfyUI result download HTTP {}",response.status()));
 }
 if response.content_length().is_some_and(|size|size>MAX_VIDEO){
  return Err("ComfyUI result is larger than 2 GiB".into());
 }
 let directory=root(settings).join("videos");
 tokio::fs::create_dir_all(&directory).await.map_err(|e|e.to_string())?;
 let part=directory.join(format!("{id}.part"));
 let final_file=directory.join(format!("{id}.{}",file.format));
 let mut out=tokio::fs::File::create(&part).await.map_err(|e|e.to_string())?;
 let mut stream=response;
 let mut total=0u64;
 let result=async{
  while let Some(chunk)=stream.chunk().await.map_err(|e|e.to_string())?{
   total=total.saturating_add(chunk.len() as u64);
   if total>MAX_VIDEO{return Err("ComfyUI video exceeds 2 GiB".to_string());}
   out.write_all(&chunk).await.map_err(|e|e.to_string())?;
  }
  out.sync_all().await.map_err(|e|e.to_string())?;
  if total<24{return Err("ComfyUI video output is empty or invalid".into());}
  let mut header=[0u8;16];
  let mut check=tokio::fs::File::open(&part).await.map_err(|e|e.to_string())?;
  check.read_exact(&mut header).await.map_err(|e|e.to_string())?;
  drop(check);
  let valid=if file.format=="mp4"{
   &header[4..8]==b"ftyp"
  }else{header.starts_with(&[0x1a,0x45,0xdf,0xa3])};
  if !valid{return Err("ComfyUI output bytes do not match MP4/WebM signature".into());}
  drop(out);
  tokio::fs::rename(&part,&final_file).await.map_err(|e|e.to_string())?;
  Ok((total,file.format.clone()))
 }.await;
 if result.is_err(){let _=tokio::fs::remove_file(&part).await;}
 result
}
pub fn output(settings:&Settings,id:Uuid)->Result<Option<(PathBuf,&'static str)>,String>{
 let Some(job)=get(settings,id)? else{return Ok(None);};
 if job.status!="completed"{return Err("ComfyUI job has not completed".into());}
 let (extension,mime)=match job.format.as_deref(){
  Some("mp4")=>("mp4","video/mp4"),Some("webm")=>("webm","video/webm"),
  _=>return Err("ComfyUI output format is invalid".into()),
 };
 let file=root(settings).join("videos").join(format!("{id}.{extension}"));
 if !file.is_file(){return Err("ComfyUI result artifact is missing".into());}
 Ok(Some((file,mime)))
}
pub fn reconcile_restart(settings:&Settings)->Result<(),String>{
 let _held=guard();
 let mut state=read_jobs(settings)?;
 let mut changed=false;
 for row in state.jobs.values_mut(){
  if matches!(row.status.as_str(),"queued"|"running"){
   row.status="needs_review".into();
   row.error=Some("Server restarted during ComfyUI execution; check remote history before submitting again".into());
   row.updated_at_ms=now_ms();changed=true;
  }
 }
 if changed{write_jobs(settings,&state)?;}
 Ok(())
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test]
 fn reject_workflow_escape_and_require_explicit_video_graph(){
  assert!(valid_url("http://127.0.0.1:8188").is_ok());
  assert!(valid_url("https://localhost:8188").is_err());
  assert!(valid_url("http://192.168.1.10:8188").is_err());
  assert!(valid_url("http://127.0.0.1:8188/admin").is_err());
  let graph=json!({
   "1":{"class_type":"CLIPTextEncode","inputs":{"text":"a test prompt"}},
   "2":{"class_type":"VHS_VideoCombine","inputs":{"images":["1",0]}}
  });
  valid_graph(&graph).unwrap();
  let cfg=Config::default();
  let rendered=fill_template(&graph,&Create{
   title:"Test".into(),brief:"new words".into(),negative:"".into(),
   aspect_ratio:"16:9".into(),duration:4,fps:16,seed:7
  },&cfg).unwrap();
  assert_eq!(rendered["1"]["inputs"]["text"],"new words");
 }
 #[test]
 fn parse_only_verified_output_video_handles(){
  let record=json!({"outputs":{"9":{"gifs":[
   {"filename":"output.mp4","subfolder":"","type":"output"},
   {"filename":"../oops.mp4","subfolder":"","type":"output"}
  ]}}});
  assert_eq!(find_video(&record).unwrap().filename,"output.mp4");
  assert!(find_video(&json!({"outputs":{"9":{"videos":[
   {"filename":"../../private.mp4","subfolder":"","type":"output"}
  ]}}})).is_none());
 }
 #[tokio::test]
 async fn queue_history_and_video_download_work_against_a_real_http_server(){
  // The test server implements ComfyUI's actual HTTP contract. It proves
  // the queued job reaches a durable video file, not only a fabricated UI row.
  let remote=Uuid::new_v4();
  let app=axum::Router::new()
   .route("/system_stats",axum::routing::get(|| async{
     axum::Json(json!({"devices":[]}))
   }))
   .route("/models/diffusion_models",axum::routing::get(||async{
     axum::Json(json!(["real-video.safetensors"]))
   }))
   .route("/models/checkpoints",axum::routing::get(||async{
     axum::Json(json!([]))
   }))
   .route("/prompt",axum::routing::post(move|axum::Json(body):axum::Json<Value>|async move{
     assert_eq!(body["prompt"]["2"]["inputs"]["unet_name"],"real-video.safetensors");
     assert_eq!(body["prompt"]["1"]["inputs"]["text"],"A real video");
     axum::Json(json!({"prompt_id":remote.to_string(),"number":0}))
   }))
   .route("/history/{id}",axum::routing::get(move||async move{
     let mut receipts=serde_json::Map::new();
     receipts.insert(remote.to_string(),json!({
      "status":{"completed":true,"status_str":"success"},
      "outputs":{"3":{"gifs":[{"filename":"proof.mp4",
        "subfolder":"","type":"output"}]}}
     }));
     axum::Json(Value::Object(receipts))
   }))
   .route("/view",axum::routing::get(||async{
     let mut mp4=vec![0,0,0,24];
     mp4.extend_from_slice(b"ftypisom000000000000");
     mp4
   }));
  let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let addr=listener.local_addr().unwrap();
  let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap();});
  let mut settings=Settings::from_env().unwrap();
  let data=std::env::temp_dir().join(format!("comfy-api-test-{}",Uuid::new_v4()));
  settings.data_dir=data.clone();
  let mut cfg=Config::default();
  cfg.engine="comfyui".into();
  cfg.comfy_url=format!("http://{addr}");
  cfg.video_model="diffusion_models/real-video.safetensors".into();
  cfg.comfy_workflow=Some(json!({
   "1":{"class_type":"CLIPTextEncode","inputs":{"text":"{{PROMPT}}"}},
   "2":{"class_type":"UNETLoader","inputs":{"unet_name":"old-model.safetensors"}},
   "3":{"class_type":"VHS_VideoCombine","inputs":{"images":["1",0],"frame_rate":16}}
  }));
  atomic(&config_path(&settings),&serde_json::to_vec(&cfg).unwrap()).unwrap();
  assert_eq!(catalog(&settings).await.len(),1);
  let result=create(settings.clone(),Create{
   title:"Real HTTP job".into(),brief:"A real video".into(),
   negative:String::new(),aspect_ratio:"16:9".into(),
   duration:3,fps:16,seed:-1
  }).await.unwrap();
  let id=Uuid::parse_str(result["id"].as_str().unwrap()).unwrap();
  let mut finished=false;
  for _ in 0..30{
   tokio::time::sleep(Duration::from_millis(200)).await;
   let item=get(&settings,id).unwrap().unwrap();
   if item.status=="completed"{finished=true;break;}
   assert_ne!(item.status,"failed","ComfyUI job failed: {:?}",item.error);
  }
  assert!(finished,"HTTP-driven ComfyUI job did not complete");
  let (file,mime)=output(&settings,id).unwrap().unwrap();
  assert_eq!(mime,"video/mp4");
  assert!(fs::metadata(file).unwrap().len()>=24);
  assert_eq!(get(&settings,id).unwrap().unwrap().prompt_id,Some(remote));
  server.abort();
  fs::remove_dir_all(data).unwrap();
 }
 #[test]
 fn in_progress_jobs_never_auto_replay_after_process_recovery(){
  let mut settings=Settings::from_env().unwrap();
  let data=std::env::temp_dir().join(format!("comfy-restart-{}",Uuid::new_v4()));
  settings.data_dir=data.clone();
  let job=Job{id:Uuid::new_v4(),prompt_id:Some(Uuid::new_v4()),
   title:"job".into(),brief:"hello".into(),aspect_ratio:"9:16".into(),
   duration:4,status:"running".into(),error:None,format:None,
   created_at_ms:now_ms(),updated_at_ms:now_ms(),bytes:0};
  write_jobs(&settings,&Journal{jobs:HashMap::from([(job.id,job.clone())])}).unwrap();
  reconcile_restart(&settings).unwrap();
  assert_eq!(get(&settings,job.id).unwrap().unwrap().status,"needs_review");
  fs::remove_dir_all(data).unwrap();
 }
}
