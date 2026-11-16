//! Autonomous, durable short-video production on the sub4 branch.
//! Script -> storyboard -> real render -> ffprobe QA -> opt-in YouTube upload.
//! Uncertain external actions never replay after a restart.
use crate::{config::Settings,domain::now_ms,engine::Engine,
 video::{NewVideo,Scene,VideoBackend,VideoStatus}};
use axum::{extract::{Path,State},http::StatusCode,routing::get,Json,Router};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::{collections::HashMap,fs,io::Write,path::{Path as FilePath,PathBuf},
 process::Stdio,sync::{Arc,OnceLock},time::Duration};
use tokio::{process::Command,sync::Mutex};
use uuid::Uuid;

type Fault=(StatusCode,Json<Value>);
fn fault(code:StatusCode,msg:impl Into<String>)->Fault{
 (code,Json(json!({"error":msg.into()})))
}
const STAGES:[&str;5]=["planning","storyboard","rendering","quality_check","publishing"];
const LIMIT:usize=12*1024*1024;
static LOCK:OnceLock<Mutex<()>>=OnceLock::new();
fn guard()->&'static Mutex<()> {LOCK.get_or_init(||Mutex::new(()))}
fn file(settings:&Settings)->PathBuf{settings.data_dir.join("video-productions/jobs.json")}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Shot{pub heading:String,pub narration:String,pub visual_prompt:String,pub seconds:u32}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Production{
 pub id:Uuid,pub topic:String,pub audience:String,pub source_notes:String,
 pub visual_engine:String,pub aspect_ratio:String,pub duration:u32,
 pub narration:bool,pub auto_publish:bool,pub privacy_status:String,
 pub status:String,pub stage:String,pub title:String,pub description:String,
 pub tags:Vec<String>,pub script:String,pub shots:Vec<Shot>,
 pub video_id:Option<Uuid>,pub upload_id:Option<Uuid>,pub upload_url:Option<String>,
 pub artifact_bytes:Option<u64>,pub measured_seconds:Option<f64>,
 pub cost_krw:f64,pub error:Option<String>,
 pub created_at_ms:u64,pub updated_at_ms:u64,
}
#[derive(Default,Serialize,Deserialize)]
struct Journal{jobs:HashMap<Uuid,Production>}
#[derive(Clone,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct NewProduction{
 pub topic:String,
 #[serde(default)]pub audience:String,
 #[serde(default)]pub source_notes:String,
 #[serde(default="native")]pub visual_engine:String,
 #[serde(default="ratio")]pub aspect_ratio:String,
 #[serde(default="seconds")]pub duration:u32,
 #[serde(default="yes")]pub narration:bool,
 #[serde(default)]pub auto_publish:bool,
 #[serde(default="private")]pub privacy_status:String,
 #[serde(default="budget")]pub max_cost_krw:f64,
}
fn native()->String{"native".into()}
fn ratio()->String{"9:16".into()}
fn seconds()->u32{30}
fn yes()->bool{true}
fn private()->String{"private".into()}
fn budget()->f64{3000.0}
fn validate(request:&NewProduction)->Result<(),String>{
 if request.topic.trim().is_empty()||request.topic.len()>600{return Err("topic must be 1-600 bytes".into());}
 if request.audience.len()>200||request.source_notes.len()>8000{return Err("audience or reference notes exceed limits".into());}
 if !matches!(request.visual_engine.as_str(),"native"|"comfyui"){return Err("visualEngine must be native or comfyui".into());}
 if !matches!(request.aspect_ratio.as_str(),"9:16"|"16:9"|"1:1"){return Err("invalid aspectRatio".into());}
 if (request.visual_engine=="native"&&!(6..=180).contains(&request.duration))||
  (request.visual_engine=="comfyui"&&!(3..=9).contains(&request.duration)){
  return Err("duration must be 6-180 seconds for native or 3-9 for ComfyUI".into());
 }
 if !matches!(request.privacy_status.as_str(),"private"|"unlisted"|"public"){return Err("invalid privacyStatus".into());}
 if !request.max_cost_krw.is_finite()||!(0.0..=1_000_000.0).contains(&request.max_cost_krw){
  return Err("maxCostKrw must be 0-1000000".into());
 }
 Ok(())
}
fn read(settings:&Settings)->Result<Journal,String>{
 let path=file(settings);
 if !path.exists(){return Ok(Journal::default());}
 let meta=fs::metadata(&path).map_err(|e|e.to_string())?;
 if meta.len()>LIMIT as u64{return Err("production journal exceeds 12 MiB".into());}
 #[cfg(unix)]{
  use std::os::unix::fs::PermissionsExt;
  if meta.permissions().mode()&0o077!=0{return Err("production journal must be owner-only".into());}
 }
 serde_json::from_slice(&fs::read(path).map_err(|e|e.to_string())?)
  .map_err(|e|format!("invalid production journal: {e}"))
}
fn write(settings:&Settings,data:&Journal)->Result<(),String>{
 if data.jobs.len()>256{return Err("production history reached 256 jobs".into());}
 let bytes=serde_json::to_vec(data).map_err(|e|e.to_string())?;
 if bytes.len()>LIMIT{return Err("production journal exceeds 12 MiB".into());}
 let path=file(settings);let dir=path.parent().ok_or("missing journal directory")?;
 fs::create_dir_all(dir).map_err(|e|e.to_string())?;
 let temp=dir.join(format!(".production-{}.tmp",Uuid::new_v4()));
 let outcome=(||->Result<(),String>{
  let mut options=fs::OpenOptions::new();options.create_new(true).write(true);
  #[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
  let mut output=options.open(&temp).map_err(|e|e.to_string())?;
  output.write_all(&bytes).map_err(|e|e.to_string())?;
  output.sync_all().map_err(|e|e.to_string())?;
  drop(output);fs::rename(&temp,&path).map_err(|e|e.to_string())?;
  #[cfg(unix)]fs::File::open(dir).and_then(|f|f.sync_all()).map_err(|e|e.to_string())?;
  Ok(())
 })();
 if outcome.is_err(){let _=fs::remove_file(temp);}
 outcome
}
async fn edit(settings:&Settings,id:Uuid,update:impl FnOnce(&mut Production))->Result<Production,String>{
 let _guard=guard().lock().await;
 let mut journal=read(settings)?;
 let record=journal.jobs.get_mut(&id).ok_or("production not found")?;
 update(record);record.updated_at_ms=now_ms();
 let snapshot=record.clone();
 write(settings,&journal)?;
 Ok(snapshot)
}
pub async fn reconcile_restart(settings:&Settings)->Result<(),String>{
 let _guard=guard().lock().await;
 let mut journal=read(settings)?;let mut changed=false;
 for record in journal.jobs.values_mut(){
  if record.status=="running"{
   record.status="needs_review".into();
   record.error=Some("Interrupted production; inspect the stored render and upload jobs. No external action was replayed.".into());
   record.updated_at_ms=now_ms();changed=true;
  }
 }
 if changed{write(settings,&journal)?;}
 Ok(())
}
pub fn public(item:&Production)->Value{
 json!({
  "id":item.id,"topic":item.topic,"audience":item.audience,
  "sourceNotesProvided":!item.source_notes.is_empty(),
  "visualEngine":item.visual_engine,"aspectRatio":item.aspect_ratio,
  "duration":item.duration,"narration":item.narration,
  "autoPublish":item.auto_publish,"privacyStatus":item.privacy_status,
  "status":item.status,"stage":item.stage,
  "stageIndex":STAGES.iter().position(|stage|*stage==item.stage).unwrap_or(0),
  "stageTotal":STAGES.len(),"stages":STAGES,
  "title":item.title,"description":item.description,"tags":item.tags,
  "script":item.script,"shots":item.shots,
  "videoId":item.video_id,"uploadId":item.upload_id,"uploadUrl":item.upload_url,
  "artifactBytes":item.artifact_bytes,"measuredSeconds":item.measured_seconds,
  "costKrw":item.cost_krw,"error":item.error,"factsVerified":false,
  "outputUrl":item.video_id.map(|id|format!("/api/video/jobs/{id}/content")),
  "createdAt":crate::frontend_state::timestamp(item.created_at_ms),
  "updatedAt":crate::frontend_state::timestamp(item.updated_at_ms)
 })
}
pub fn routes()->Router<Arc<Engine>>{
 Router::new().route("/video/productions",get(list).post(create))
  .route("/video/productions/{id}",get(get_production))
}
async fn list(State(engine):State<Arc<Engine>>)->Result<Json<Value>,Fault>{
 let records=read(&engine.settings).map_err(|e|fault(StatusCode::INTERNAL_SERVER_ERROR,e))?;
 let mut sorted=records.jobs.into_values().collect::<Vec<_>>();
 sorted.sort_by_key(|job|std::cmp::Reverse(job.created_at_ms));
 Ok(Json(json!(sorted.iter().map(public).collect::<Vec<_>>())))
}
async fn get_production(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)->Result<Json<Value>,Fault>{
 let record=read(&engine.settings).map_err(|e|fault(StatusCode::INTERNAL_SERVER_ERROR,e))?
  .jobs.remove(&id).ok_or_else(||fault(StatusCode::NOT_FOUND,"production not found"))?;
 Ok(Json(public(&record)))
}
async fn create(State(engine):State<Arc<Engine>>,Json(input):Json<NewProduction>)
 ->Result<(StatusCode,Json<Value>),Fault>{
 validate(&input).map_err(|e|fault(StatusCode::BAD_REQUEST,e))?;
 if input.auto_publish&&!crate::frontend_oauth::youtube_connection(&engine.settings){
  return Err(fault(StatusCode::PRECONDITION_REQUIRED,"Connect YouTube before enabling automatic upload"));
 }
 if input.visual_engine=="comfyui"{
  let cfg=crate::frontend_comfy::config(&engine.settings)
   .map_err(|e|fault(StatusCode::BAD_REQUEST,e))?;
  if cfg.engine!="comfyui"||cfg.comfy_workflow.is_none()||
   !crate::frontend_comfy::reachable(&cfg).await{
   return Err(fault(StatusCode::PRECONDITION_REQUIRED,
    "Select and connect ComfyUI with an API workflow before autonomous rendering"));
  }
 }
 let now=now_ms();
 let job=Production{
  id:Uuid::new_v4(),topic:input.topic.trim().into(),
  audience:input.audience.trim().into(),source_notes:input.source_notes.trim().into(),
  visual_engine:input.visual_engine.clone(),aspect_ratio:input.aspect_ratio.clone(),
  duration:input.duration,narration:input.narration,auto_publish:input.auto_publish,
  privacy_status:input.privacy_status.clone(),status:"running".into(),stage:"planning".into(),
  title:String::new(),description:String::new(),tags:Vec::new(),script:String::new(),
  shots:Vec::new(),video_id:None,upload_id:None,upload_url:None,
  artifact_bytes:None,measured_seconds:None,cost_krw:0.0,error:None,
  created_at_ms:now,updated_at_ms:now
 };
 {
  let _guard=guard().lock().await;
  let mut journal=read(&engine.settings).map_err(|e|fault(StatusCode::INTERNAL_SERVER_ERROR,e))?;
  if journal.jobs.len()>=256{return Err(fault(StatusCode::CONFLICT,"production journal is full"));}
  journal.jobs.insert(job.id,job.clone());
  write(&engine.settings,&journal).map_err(|e|fault(StatusCode::INTERNAL_SERVER_ERROR,e))?;
 }
 let worker=Arc::clone(&engine);
 let id=job.id;
 tokio::spawn(async move{
  if let Err(reason)=execute(&worker,id,input).await{
   if let Err(error)=edit(&worker.settings,id,|record|{
    record.status="failed".into();record.error=Some(reason);
   }).await {tracing::error!(%error,production=%id,"unable to persist failed production");}
  }
 });
 Ok((StatusCode::ACCEPTED,Json(public(&job))))
}
#[derive(Deserialize)]
struct Script{title:String,description:String,#[serde(default)]tags:Vec<String>,scenes:Vec<Proposal>}
#[derive(Deserialize)]
struct Proposal{heading:String,narration:String,#[serde(default)]visual_prompt:String}
fn storyboard(text:&str,duration:u32,comfy:bool)
 ->Result<(String,String,Vec<String>,Vec<Shot>),String>{
 let data:Script=serde_json::from_str(text.trim())
  .map_err(|e|format!("storyboard must be strict JSON: {e}"))?;
 let count=data.scenes.len();
 if count==0||count>12||(comfy&&count!=1)||duration<(count as u32)*2{
  return Err("storyboard scene count does not match requested duration".into());
 }
 if data.title.trim().is_empty(){return Err("storyboard title missing".into());}
 let base=duration/count as u32;
 let rest=duration%count as u32;
 let mut shots=Vec::with_capacity(count);
 for (index,shot) in data.scenes.into_iter().enumerate(){
  let sec=base+u32::from((index as u32)<rest);
  if !(2..=30).contains(&sec)||shot.heading.trim().is_empty()||
   shot.heading.len()>180||shot.narration.len()>1000||shot.visual_prompt.len()>1400{
   return Err("invalid storyboard scene, text length or clip duration".into());
  }
  let visual=if shot.visual_prompt.trim().is_empty(){
   shot.heading.clone()
  }else{shot.visual_prompt.trim().into()};
  shots.push(Shot{heading:shot.heading.trim().into(),narration:shot.narration.trim().into(),
   visual_prompt:visual,seconds:sec});
 }
 Ok((data.title.chars().take(100).collect(),
  data.description.chars().take(4000).collect(),
  data.tags.into_iter().filter(|tag|!tag.trim().is_empty())
   .take(12).map(|tag|tag.chars().take(45).collect()).collect(),shots))
}
async fn execute(engine:&Arc<Engine>,id:Uuid,options:NewProduction)->Result<(),String>{
 let settings=&engine.settings;
 edit(settings,id,|p|p.stage="storyboard".into()).await?;
 let count=if options.visual_engine=="comfyui"{1}else{(options.duration/8).clamp(2,6)};
 let prompt=vec![
  json!({"role":"system","content":format!(
   "You are an autonomous video producer. Output ONLY valid JSON with title, description, tags (array), scenes (array of EXACTLY {count} objects containing heading, narration, visual_prompt). Script must have a hook, structure and ending. Use the user's language. Never fabricate research, sources, quotes, or statistics. When notes are provided ground factual claims only in supplied notes; otherwise use expressive fiction or general explanations. Visual prompts must describe actual shots. No markdown."
  )}),
  json!({"role":"user","content":format!(
   "Topic: {}\nAudience: {}\nReference notes (user supplied and not independently verified): {}\nDuration: {} seconds\nAspect: {}\nVisual engine: {}\nScene count: {count}",
   options.topic,options.audience,
   if options.source_notes.is_empty(){"none"}else{&options.source_notes},
   options.duration,options.aspect_ratio,options.visual_engine
  )})
 ];
 let response=engine.model.complete(&prompt,&[],false).await?;
 let (title,description,tags,shots)=storyboard(
  &response.text,options.duration,options.visual_engine=="comfyui")?;
 let cost=response.cost_krw;
 edit(settings,id,|p|{
  p.title=title.clone();p.description=description.clone();p.tags=tags.clone();
  p.script=shots.iter().map(|s|s.narration.as_str()).collect::<Vec<_>>().join("\n\n");
  p.shots=shots.clone();p.cost_krw=cost;
 }).await?;
 if cost>options.max_cost_krw{return Err(format!(
  "AI script cost {cost:.2} KRW exceeded the {:.2} KRW production limit; no video rendering started",
  options.max_cost_krw));}
 edit(settings,id,|p|p.stage="rendering".into()).await?;
 let video_id=if options.visual_engine=="native"{
  let scenes=shots.iter().map(|s|Scene{
   heading:s.heading.clone(),narration:s.narration.clone(),seconds:s.seconds
  }).collect();
  let record=engine.videos.create(&engine.model,NewVideo{
   prompt:options.topic.clone(),scenes:Some(scenes),
   backend:Some(VideoBackend::Local),aspect_ratio:Some(options.aspect_ratio.clone())
  }).await?;
  engine.videos.set_title(record.id,title.clone()).await?;
  engine.videos.set_narration(record.id,options.narration).await?;
  edit(settings,id,|p|p.video_id=Some(record.id)).await?;
  engine.videos.start(record.id).await?;
  record.id
 }else{
  let record=crate::frontend_comfy::create(settings.clone(),crate::frontend_comfy::Create{
   title:title.clone(),brief:shots[0].visual_prompt.clone(),negative:String::new(),
   aspect_ratio:options.aspect_ratio.clone(),duration:options.duration,fps:16,seed:-1
  }).await?;
  let video_id=Uuid::parse_str(record["id"].as_str().ok_or("ComfyUI job ID missing")?)
   .map_err(|e|e.to_string())?;
  edit(settings,id,|p|p.video_id=Some(video_id)).await?;
  video_id
 };
 tokio::time::timeout(Duration::from_secs(2400),async{
  loop{
   tokio::time::sleep(Duration::from_secs(2)).await;
   let (status,reason)=if options.visual_engine=="native"{
    let v=engine.videos.get(video_id).await.ok_or("native render not found")?;
    (match v.status{
     VideoStatus::Completed=>"completed",VideoStatus::Failed=>"failed",_=>"running"
    }.to_owned(),v.error)
   }else{
    let v=crate::frontend_comfy::get(settings,video_id)?
     .ok_or("ComfyUI render not found")?;
    (v.status,v.error)
   };
   match status.as_str(){
    "completed"=>return Ok::<(),String>(()),
    "failed"|"needs_review"|"interrupted"=>return Err(
     reason.unwrap_or_else(||"renderer failed".into())),
    _=>{}
   }
  }
 }).await.map_err(|_|"renderer exceeded production deadline; check original render before retrying".to_owned())??;
 edit(settings,id,|p|p.stage="quality_check".into()).await?;
 let output=if options.visual_engine=="native"{
  engine.videos.output_path(video_id).await?
 }else{
  crate::frontend_comfy::output(settings,video_id)?
   .ok_or("ComfyUI output artifact missing")?.0
 };
 let (bytes,duration)=inspect(&output,options.duration).await?;
 edit(settings,id,|p|{
  p.artifact_bytes=Some(bytes);p.measured_seconds=Some(duration);
 }).await?;
 if !options.auto_publish{
  edit(settings,id,|p|p.status="ready".into()).await?;
  return Ok(());
 }
 edit(settings,id,|p|p.stage="publishing".into()).await?;
 let upload=crate::frontend_youtube::queue_upload(
  Arc::clone(engine),video_id,&title,&description,&options.privacy_status,&tags.join(",")
 ).await?;
 let upload_id=Uuid::parse_str(upload["id"].as_str().ok_or("YouTube job ID missing")?)
  .map_err(|e|e.to_string())?;
 edit(settings,id,|p|p.upload_id=Some(upload_id)).await?;
 tokio::time::timeout(Duration::from_secs(1800),async{
  loop{
   tokio::time::sleep(Duration::from_secs(3)).await;
   let row=crate::frontend_youtube::lookup_upload(settings,upload_id)?
    .ok_or("YouTube upload disappeared")?;
   match row["status"].as_str().unwrap_or(""){
    "completed"=>{
     let url=row["result"]["url"].as_str()
      .ok_or("YouTube completed without result URL")?.to_owned();
     edit(settings,id,|p|{p.upload_url=Some(url);p.status="published".into();}).await?;
     return Ok::<(),String>(());
    },
    "failed"|"needs_review"=>{
     return Err(row["error"].as_str().unwrap_or("YouTube upload failed").to_owned());
    },
    _=>{}
   }
  }
 }).await.map_err(|_|"YouTube status timeout; inspect upload journal before retrying".to_owned())?
}
async fn inspect(path:&FilePath,expected:u32)->Result<(u64,f64),String>{
 let bytes=tokio::fs::metadata(path).await.map_err(|e|e.to_string())?.len();
 if bytes<1024||bytes>2*1024*1024*1024{return Err("invalid video artifact size".into());}
 let cmd=Command::new("ffprobe").arg("-v").arg("error")
  .arg("-show_entries").arg("format=duration:stream=codec_type")
  .arg("-of").arg("json").arg(path)
  .stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true).output();
 let output=tokio::time::timeout(Duration::from_secs(30),cmd)
  .await.map_err(|_|"ffprobe timeout")?
  .map_err(|e|format!("ffprobe unavailable: {e}"))?;
 if !output.status.success(){
  return Err(format!("ffprobe rejected video: {}",String::from_utf8_lossy(&output.stderr)
   .chars().take(300).collect::<String>()));
 }
 if output.stdout.len()>64_000{return Err("ffprobe output too large".into());}
 let data:Value=serde_json::from_slice(&output.stdout).map_err(|e|e.to_string())?;
 if !data["streams"].as_array().is_some_and(|streams|
  streams.iter().any(|s|s["codec_type"]=="video")){
  return Err("artifact has no video stream".into());
 }
 let duration=data["format"]["duration"].as_str().ok_or("ffprobe duration missing")?
  .parse::<f64>().map_err(|e|e.to_string())?;
 if !duration.is_finite()||duration<(expected as f64)*0.65||duration>(expected as f64)+6.0{
  return Err(format!("rendered duration {duration:.1}s mismatches requested {expected}s"));
 }
 Ok((bytes,duration))
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test]
 fn defaults_keep_upload_opt_in_and_private(){
  let input:NewProduction=serde_json::from_value(json!({"topic":"a scenic city"})).unwrap();
  assert!(!input.auto_publish);assert_eq!(input.privacy_status,"private");
  validate(&input).unwrap();
  let mut bad=input.clone();bad.privacy_status="unsupported".into();
  assert!(validate(&bad).is_err());
  bad=input;bad.visual_engine="comfyui".into();
  assert!(validate(&bad).is_err());
 }
 #[test]
 fn AI_storyboard_gets_exact_duration_from_orchestrator(){
  let raw=json!({"title":"City","description":"A story","tags":["design"],
   "scenes":[{"heading":"Start","narration":"One","visual_prompt":"An avenue"},
    {"heading":"End","narration":"Two","visual_prompt":"A skyline"}]}).to_string();
  let (_,_,_,shots)=storyboard(&raw,13,false).unwrap();
  assert_eq!(shots.iter().map(|s|s.seconds).sum::<u32>(),13);
  assert!(storyboard(&raw,8,true).is_err());
 }
 #[tokio::test]
 async fn crash_does_not_replay_upload(){
  let dir=std::env::temp_dir().join(format!("video-production-{}",Uuid::new_v4()));
  let mut settings=Settings::from_env().unwrap();settings.data_dir=dir.clone();
  let entry=Production{
   id:Uuid::new_v4(),topic:"test".into(),audience:String::new(),
   source_notes:String::new(),visual_engine:"native".into(),
   aspect_ratio:"9:16".into(),duration:10,narration:true,
   auto_publish:true,privacy_status:"private".into(),status:"running".into(),
   stage:"publishing".into(),title:"Test".into(),description:String::new(),
   tags:vec![],script:String::new(),shots:vec![],video_id:Some(Uuid::new_v4()),
   upload_id:Some(Uuid::new_v4()),upload_url:None,artifact_bytes:None,
   measured_seconds:None,cost_krw:0.0,error:None,created_at_ms:now_ms(),updated_at_ms:now_ms()
  };
  write(&settings,&Journal{jobs:HashMap::from([(entry.id,entry.clone())])}).unwrap();
  reconcile_restart(&settings).await.unwrap();
  let current=read(&settings).unwrap().jobs.remove(&entry.id).unwrap();
  assert_eq!(current.status,"needs_review");
  assert_eq!(current.upload_id,entry.upload_id);
  fs::remove_dir_all(dir).unwrap();
 }
}
