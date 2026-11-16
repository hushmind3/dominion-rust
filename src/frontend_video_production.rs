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
 #[serde(default)]pub visual_bible:String,
 #[serde(default)]pub script_attempts:u8,
 pub video_id:Option<Uuid>,pub upload_id:Option<Uuid>,pub upload_url:Option<String>,
 #[serde(default)]pub rendered_shot_ids:Vec<Uuid>,
 #[serde(default)]pub completed_shots:usize,
 #[serde(default)]pub artifact_revision_id:Option<Uuid>,
 #[serde(default)]pub revision_count:u32,
 #[serde(default)]pub regenerating_shot:Option<usize>,
 #[serde(default)]pub pending_revision_job_id:Option<Uuid>,
 #[serde(default)]pub revision_error:Option<String>,
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
  (request.visual_engine=="comfyui"&&!(3..=90).contains(&request.duration)){
  return Err("duration must be 6-180 seconds for native or 3-90 for ComfyUI".into());
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
  if record.regenerating_shot.take().is_some(){
   record.pending_revision_job_id=None;
   record.revision_error=Some("Scene revision interrupted; the previous verified MP4 is preserved. Inspect the previous ComfyUI job before trying again.".into());
   record.updated_at_ms=now_ms();changed=true;
  }
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
  "visualBible":item.visual_bible,"scriptAttempts":item.script_attempts,
  "videoId":item.video_id,"uploadId":item.upload_id,"uploadUrl":item.upload_url,
  "artifactBytes":item.artifact_bytes,"measuredSeconds":item.measured_seconds,
  "renderedShotIds":item.rendered_shot_ids,"completedShots":item.completed_shots,
  "artifactRevisionId":item.artifact_revision_id,"revisionCount":item.revision_count,
  "regeneratingShot":item.regenerating_shot,"revisionError":item.revision_error,
  "costKrw":item.cost_krw,"error":item.error,"factsVerified":false,
  "outputUrl":item.video_id.map(|id|format!("/api/video/jobs/{id}/content")),
  "createdAt":crate::frontend_state::timestamp(item.created_at_ms),
  "updatedAt":crate::frontend_state::timestamp(item.updated_at_ms)
 })
}
pub fn routes()->Router<Arc<Engine>>{
 Router::new().route("/video/productions",get(list).post(create))
  .route("/video/productions/{id}",get(get_production))
  .route("/video/productions/{id}/shots/{index}/regenerate",
   axum::routing::post(regenerate_shot))
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
  // Never spend expensive GPU time if there is no usable final MP4 assembler.
  if input.visual_engine=="comfyui"{
   let encoders=tokio::time::timeout(Duration::from_secs(6),
    Command::new("ffmpeg").args(["-hide_banner","-encoders"])
     .stdout(Stdio::piped()).stderr(Stdio::null())
     .kill_on_drop(true).output()).await.ok().and_then(Result::ok);
   let probe=tokio::time::timeout(Duration::from_secs(6),
    Command::new("ffprobe").arg("-version")
     .stdout(Stdio::null()).stderr(Stdio::null())
     .kill_on_drop(true).output()).await.ok().and_then(Result::ok);
   if !encoders.is_some_and(|out|out.status.success()&&
       String::from_utf8_lossy(&out.stdout).contains("libx264"))||
      !probe.is_some_and(|out|out.status.success()){
    return Err(fault(StatusCode::PRECONDITION_REQUIRED,
     "FFmpeg with libx264 and ffprobe are required for autonomous ComfyUI video assembly"));
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
  shots:Vec::new(),visual_bible:String::new(),script_attempts:0,video_id:None,upload_id:None,upload_url:None,
  rendered_shot_ids:Vec::new(),completed_shots:0,
  artifact_revision_id:None,revision_count:0,regenerating_shot:None,
  pending_revision_job_id:None,revision_error:None,
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
    // Once publication starts, an HTTP timeout or crash does not prove
    // whether YouTube accepted the upload. Never label that safe to retry.
    record.status=if record.stage=="publishing"||
     (record.stage=="rendering"&&record.visual_engine=="comfyui"&&
      record.rendered_shot_ids.len()>record.completed_shots){
     "needs_review"
    }else{"failed"}.into();
    record.error=Some(reason);
   }).await {tracing::error!(%error,production=%id,"unable to persist failed production");}
  }
 });
 Ok((StatusCode::ACCEPTED,Json(public(&job))))
}
#[derive(Deserialize)]
struct Script{title:String,description:String,#[serde(default)]tags:Vec<String>,
 #[serde(default)]visual_bible:String,scenes:Vec<Proposal>}
#[derive(Deserialize)]
struct Proposal{heading:String,narration:String,#[serde(default)]visual_prompt:String}
fn storyboard(text:&str,duration:u32,comfy:bool)
 ->Result<(String,String,Vec<String>,Vec<Shot>,String),String>{
 // Some OpenAI-compatible models wrap valid JSON in a markdown fence.
 // Accept only a single complete JSON document, never substring extraction.
 let trimmed=text.trim();
 let normalized=if trimmed.starts_with(char::from(96)){
  trimmed.lines().skip(1).take_while(|line|!line.starts_with(char::from(96)))
   .collect::<Vec<_>>().join("\n")
 }else{trimmed.to_owned()};
 let data:Script=serde_json::from_str(normalized.trim())
  .map_err(|e|format!("storyboard must be valid JSON: {e}"))?;
 let count=data.scenes.len();
 if count==0||count>12||duration<(count as u32)*2{
  return Err("storyboard scene count does not match requested duration".into());
 }
 if data.title.trim().is_empty(){return Err("storyboard title missing".into());}
 if data.visual_bible.len()>900{return Err("visual bible exceeds 900 bytes".into());}
 let base=duration/count as u32;
 let rest=duration%count as u32;
 let mut shots=Vec::with_capacity(count);
 for (index,shot) in data.scenes.into_iter().enumerate(){
  let sec=base+u32::from((index as u32)<rest);
  if !(if comfy{3..=9}else{2..=30}).contains(&sec)||shot.heading.trim().is_empty()||
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
   .take(12).map(|tag|tag.chars().take(45).collect()).collect(),shots,
  data.visual_bible.trim().to_owned()))
}
async fn execute(engine:&Arc<Engine>,id:Uuid,options:NewProduction)->Result<(),String>{
 let settings=&engine.settings;
 edit(settings,id,|p|p.stage="storyboard".into()).await?;
 let count=if options.visual_engine=="comfyui"{options.duration.div_ceil(8)}else{(options.duration/8).clamp(2,6)};
 let prompt=vec![
  json!({"role":"system","content":format!(
   "You are an autonomous video producer. Output ONLY valid JSON with title, description, tags (array), visual_bible (one stable character, art direction, lighting, wardrobe and color palette; <=900 bytes), scenes (array of EXACTLY {count} objects containing heading, narration, visual_prompt). Every scene must preserve the visual_bible while introducing a distinct shot and motion. Script must have a hook, structure and ending. Use the user's language. Never fabricate research, sources, quotes, or statistics. When notes are provided ground factual claims only in supplied notes; otherwise use expressive fiction or general explanations. Visual prompts must describe actual shots. No markdown."
  )}),
  json!({"role":"user","content":format!(
   "Topic: {}\nAudience: {}\nReference notes (user supplied and not independently verified): {}\nDuration: {} seconds\nAspect: {}\nVisual engine: {}\nScene count: {count}",
   options.topic,options.audience,
   if options.source_notes.is_empty(){"none"}else{&options.source_notes},
   options.duration,options.aspect_ratio,options.visual_engine
  )})
 ];
 // Schema violations receive bounded corrective turns on the same model,
 // while each additional attempt is charged to the persisted script budget.
 let mut total_cost=0.0;
 let mut attempts=0u8;
 let (title,description,tags,shots,visual_bible)=loop{
  attempts+=1;
  let response=engine.model.complete(&prompt,&[],false).await?;
  total_cost+=response.cost_krw.max(0.0);
  edit(settings,id,|p|{p.cost_krw=total_cost;p.script_attempts=attempts;}).await?;
  if total_cost>options.max_cost_krw{return Err(format!(
   "AI storyboard cost {total_cost:.2} KRW exceeded the {:.2} KRW limit; rendering was not started",
   options.max_cost_krw));}
  match storyboard(&response.text,options.duration,options.visual_engine=="comfyui"){
   Ok(valid)=>break valid,
   Err(problem) if attempts<3&&total_cost<options.max_cost_krw=>{
    // Preserve the original system constraint, never execute model text.
    // Cap the bad response included in the repair context.
    let excerpt=response.text.chars().take(1800).collect::<String>();
    prompt.push(json!({"role":"assistant","content":excerpt}));
    prompt.push(json!({"role":"user","content":format!(
     "The prior output did not satisfy the JSON storyboard contract: {problem}. \
Return a corrected COMPLETE JSON document with exactly {count} scenes \
whose combined duration is {} seconds. No markdown.",
     options.duration)}));
   },
   Err(problem)=>return Err(format!(
    "storyboard failed validation after {attempts} attempt(s): {problem}")),
  }
 };
 edit(settings,id,|p|{
  p.title=title.clone();p.description=description.clone();p.tags=tags.clone();
  p.script=shots.iter().map(|s|s.narration.as_str()).collect::<Vec<_>>().join("\n\n");
  p.shots=shots.clone();p.visual_bible=visual_bible.clone();
 }).await?;
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
   let native_id=record.id;
   tokio::time::timeout(Duration::from_secs(2400),async{
    loop{
     tokio::time::sleep(Duration::from_secs(2)).await;
     let v=engine.videos.get(native_id).await.ok_or("native render not found")?;
     match v.status{
      VideoStatus::Completed=>return Ok::<(),String>(()),
      VideoStatus::Failed=>return Err(v.error.unwrap_or_else(||"native renderer failed".into())),
      _=>{}
     }
    }
   }).await.map_err(|_|"native renderer deadline reached; inspect original render".to_owned())??;
   native_id
  }else{
   // Every remote scene ID is journaled before polling. An interrupted
   // generation must not be submitted again without user reconciliation.
   let mut clips=Vec::with_capacity(shots.len());
   for (index,shot) in shots.iter().enumerate(){
    let record=crate::frontend_comfy::create(settings.clone(),crate::frontend_comfy::Create{
     title:format!("{} · {}/{}",title,index+1,shots.len()),
     brief:continuity_prompt(&visual_bible,&shot.visual_prompt),negative:String::new(),
     aspect_ratio:options.aspect_ratio.clone(),duration:shot.seconds,fps:16,seed:-1
    }).await?;
    let clip_id=Uuid::parse_str(record["id"].as_str().ok_or("ComfyUI job ID missing")?)
     .map_err(|e|e.to_string())?;
    edit(settings,id,|p|p.rendered_shot_ids.push(clip_id)).await?;
    tokio::time::timeout(Duration::from_secs(2400),async{
     loop{
      tokio::time::sleep(Duration::from_secs(2)).await;
      let job=crate::frontend_comfy::get(settings,clip_id)?
       .ok_or("ComfyUI scene render not found")?;
      match job.status.as_str(){
       "completed"=>return Ok::<(),String>(()),
       "failed"|"needs_review"|"interrupted"=>
        return Err(job.error.unwrap_or_else(||"ComfyUI scene failed".into())),
       _=>{}
      }
     }
    }).await.map_err(|_|format!("ComfyUI scene {} timed out; inspect remote job before retrying",index+1))??;
    let clip=crate::frontend_comfy::output(settings,clip_id)?
     .ok_or("Completed ComfyUI scene is missing its video artifact")?.0;
    inspect(&clip,shot.seconds).await?;
    clips.push((clip,shot.seconds));
    edit(settings,id,|p|p.completed_shots=index+1).await?;
   }
   assemble_clips(settings,id,&clips,&options.aspect_ratio).await?;
   // The finished production ID identifies the assembled output, never
   // just the last raw generated scene.
   edit(settings,id,|p|p.video_id=Some(id)).await?;
   id
  };
 edit(settings,id,|p|p.stage="quality_check".into()).await?;
 let output=if options.visual_engine=="native"{
   engine.videos.output_path(video_id).await?
  }else{
   assembled_path(settings,id)
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

/// Reuse a shared visual identity and art direction for all scene prompts.
/// This is textual continuity, not keyframe/image identity conditioning.
fn continuity_prompt(visual_bible:&str,shot:&str)->String{
 if visual_bible.is_empty(){return shot.to_owned();}
 format!("Maintain these visual identities and art direction in every shot: {visual_bible}\nScene: {shot}")
}

fn assembled_path(settings:&Settings,id:Uuid)->PathBuf{
 settings.data_dir.join("video-productions/outputs").join(format!("{id}.mp4"))
}

/// Resolve only completed, verified productions. Historic single-scene
/// ComfyUI outputs continue resolving through their own clip identifiers.
pub fn output(settings:&Settings,id:Uuid)->Result<Option<(PathBuf,&'static str)>,String>{
 let Some(job)=read(settings)?.jobs.get(&id).cloned()else{return Ok(None)};
 if job.visual_engine!="comfyui"{return Ok(None);}
 let ready=matches!(job.status.as_str(),"ready"|"published")||
  (job.stage=="publishing"&&job.artifact_bytes.is_some()&&
   matches!(job.status.as_str(),"running"|"needs_review"));
 if !ready{return Err("assembled production is not ready for export".into());}
 let path=assembled_path(settings,job.artifact_revision_id.unwrap_or(id));
 if !path.is_file(){return Err("assembled production artifact is missing".into());}
 Ok(Some((path,"video/mp4")))
}

pub fn finished(settings:&Settings)->Result<Vec<Value>,String>{
 let data=read(settings)?;
 Ok(data.jobs.values().filter(|p|
  p.visual_engine=="comfyui"&&matches!(p.status.as_str(),"ready"|"published")&&
  assembled_path(settings,p.artifact_revision_id.unwrap_or(p.id)).is_file()
 ).map(|p|json!({
  "id":p.id,"kind":"video","status":"completed","title":p.title,
  "brief":p.topic,"createdAt":crate::frontend_state::timestamp(p.created_at_ms),
  "updatedAt":crate::frontend_state::timestamp(p.updated_at_ms),
  "outputUrl":format!("/api/video/jobs/{}/content",p.id),
  "duration":p.duration,"aspectRatio":p.aspect_ratio,"costKrw":p.cost_krw,
  "result":{"bytes":p.artifact_bytes},
  "artifactRevisionId":p.artifact_revision_id
 })).collect())
}

/// Normalize mixed source formats before stitching, then atomically expose
/// only an ffprobe-verified MP4. FFmpeg is invoked without a shell.
async fn assemble_clips(settings:&Settings,id:Uuid,clips:&[(PathBuf,u32)],ratio:&str)->Result<(),String>{
 if clips.is_empty()||clips.len()>12{return Err("invalid ComfyUI scene count".into());}
 let (width,height)=match ratio{
  "9:16"=>(720,1280),"16:9"=>(1280,720),"1:1"=>(720,720),
  _=>return Err("invalid production aspect ratio".into())
 };
 let destination=assembled_path(settings,id);
 let directory=destination.parent().ok_or("missing assembly directory")?;
 tokio::fs::create_dir_all(directory).await.map_err(|e|e.to_string())?;
 let temporary=directory.join(format!("{id}.part.mp4"));
 let mut command=Command::new("ffmpeg");
 command.args(["-hide_banner","-nostdin","-loglevel","error","-y"]);
 for (source,_) in clips{command.arg("-i").arg(source);}
 let mut filters=Vec::with_capacity(clips.len()+1);
 for (index,(_,seconds)) in clips.iter().enumerate(){
  filters.push(format!(
   "[{index}:v]scale={width}:{height}:force_original_aspect_ratio=decrease,\
pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:color=black,\
setsar=1,fps=24,format=yuv420p,trim=duration={seconds},setpts=PTS-STARTPTS[v{index}]"
  ));
 }
 let inputs=(0..clips.len()).map(|i|format!("[v{i}]")).collect::<String>();
 filters.push(format!("{inputs}concat=n={}:v=1:a=0[vout]",clips.len()));
 command.arg("-filter_complex").arg(filters.join(";"))
  .args(["-map","[vout]","-c:v","libx264","-preset","veryfast",
   "-crf","23","-pix_fmt","yuv420p","-movflags","+faststart","-an"])
  .arg(&temporary)
  .stdout(Stdio::null()).stderr(Stdio::piped()).kill_on_drop(true);
 let outcome=tokio::time::timeout(Duration::from_secs(900),command.output())
  .await.map_err(|_|"FFmpeg scene assembly exceeded 15 minutes".to_owned())?
  .map_err(|e|format!("FFmpeg assembly unavailable: {e}"))?;
 let result=async{
  if !outcome.status.success(){
   return Err(format!("FFmpeg scene assembly failed: {}",
    String::from_utf8_lossy(&outcome.stderr).chars().take(500).collect::<String>()));
  }
  inspect(&temporary,clips.iter().map(|(_,seconds)|seconds).sum()).await?;
  tokio::fs::rename(&temporary,&destination).await.map_err(|e|e.to_string())?;
  Ok::<(),String>(())
 }.await;
 if result.is_err(){let _=tokio::fs::remove_file(&temporary).await;}
 result
}


#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct ReviseShot{visual_prompt:String}

/// Re-render exactly one already-approved, completed scene. The previous MP4
/// stays readable until a new immutable revision is independently validated.
async fn regenerate_shot(State(engine):State<Arc<Engine>>,
 Path((id,index)):Path<(Uuid,usize)>,Json(input):Json<ReviseShot>)
 ->Result<(StatusCode,Json<Value>),Fault>{
 if input.visual_prompt.trim().is_empty()||input.visual_prompt.len()>1400{
  return Err(fault(StatusCode::BAD_REQUEST,"visualPrompt must be 1-1400 bytes"));
 }
 let snapshot={
  let _held=guard().lock().await;
  let mut records=read(&engine.settings).map_err(|e|fault(StatusCode::INTERNAL_SERVER_ERROR,e))?;
  let job=records.jobs.get_mut(&id).ok_or_else(||fault(StatusCode::NOT_FOUND,"production not found"))?;
  if job.visual_engine!="comfyui"||job.status!="ready"||
   job.shots.get(index).is_none()||job.rendered_shot_ids.len()!=job.shots.len()||
   job.regenerating_shot.is_some()||job.completed_shots!=job.shots.len(){
   return Err(fault(StatusCode::CONFLICT,
    "scene revision requires a completed, unpublished ComfyUI production with all source clips available"));
  }
  if !assembled_path(&engine.settings,job.artifact_revision_id.unwrap_or(id)).is_file(){
   return Err(fault(StatusCode::CONFLICT,"verified master video is missing"));
  }
  job.regenerating_shot=Some(index);
  job.pending_revision_job_id=None;
  job.revision_error=None;job.updated_at_ms=now_ms();
  let snapshot=job.clone();
  write(&engine.settings,&records).map_err(|e|fault(StatusCode::INTERNAL_SERVER_ERROR,e))?;
  snapshot
 };
 let settings=engine.settings.clone();
 let worker=Arc::clone(&engine);
 tokio::spawn(async move{
  if let Err(problem)=execute_shot_revision(&worker,id,index,input.visual_prompt).await{
   if let Err(storage)=edit(&settings,id,|job|{
    job.regenerating_shot=None;job.pending_revision_job_id=None;
    job.revision_error=Some(problem);
   }).await{tracing::error!(%storage,production=%id,"failed to persist scene revision error");}
  }
 });
 Ok((StatusCode::ACCEPTED,Json(public(&snapshot))))
}

async fn execute_shot_revision(engine:&Arc<Engine>,id:Uuid,index:usize,prompt:String)->Result<(),String>{
 let settings=&engine.settings;
 let production=read(settings)?.jobs.get(&id).cloned().ok_or("production missing")?;
 let shot=production.shots.get(index).ok_or("scene missing")?;
 let created=crate::frontend_comfy::create(settings.clone(),crate::frontend_comfy::Create{
  title:format!("{} · revision {}",production.title,index+1),
  brief:continuity_prompt(&production.visual_bible,&prompt),
  negative:String::new(),aspect_ratio:production.aspect_ratio.clone(),
  duration:shot.seconds,fps:16,seed:-1
 }).await?;
 let clip_id=Uuid::parse_str(created["id"].as_str().ok_or("ComfyUI scene ID missing")?)
  .map_err(|e|e.to_string())?;
 edit(settings,id,|job|job.pending_revision_job_id=Some(clip_id)).await?;
 tokio::time::timeout(Duration::from_secs(2400),async{
  loop{
   tokio::time::sleep(Duration::from_secs(2)).await;
   let clip=crate::frontend_comfy::get(settings,clip_id)?.ok_or("revised clip missing")?;
   match clip.status.as_str(){
    "completed"=>return Ok::<(),String>(()),
    "failed"|"interrupted"|"needs_review"=>
     return Err(clip.error.unwrap_or("revised ComfyUI scene failed".into())),
    _=>{}
   }
  }
 }).await.map_err(|_|"revised scene timeout; inspect remote job before retrying".to_owned())??;
 let new_clip=crate::frontend_comfy::output(settings,clip_id)?
  .ok_or("revised scene video file is missing")?.0;
 inspect(&new_clip,shot.seconds).await?;
 let mut clips=Vec::with_capacity(production.shots.len());
 for (i,existing) in production.rendered_shot_ids.iter().enumerate(){
  let path=if i==index{new_clip.clone()}else{
   crate::frontend_comfy::output(settings,*existing)?
    .ok_or_else(||format!("source scene {} no longer available",i+1))?.0
  };
  clips.push((path,production.shots[i].seconds));
 }
 // Each accepted revision gets a fresh immutable artifact. On any failure,
 // the journal pointer continues to reference the previously verified MP4.
 let revision=Uuid::new_v4();
 assemble_clips(settings,revision,&clips,&production.aspect_ratio).await?;
 let (bytes,seconds)=inspect(&assembled_path(settings,revision),production.duration).await?;
 edit(settings,id,|job|{
  job.artifact_revision_id=Some(revision);
  job.rendered_shot_ids[index]=clip_id;
  job.shots[index].visual_prompt=prompt;
  job.artifact_bytes=Some(bytes);job.measured_seconds=Some(seconds);
  job.revision_count=job.revision_count.saturating_add(1);
  job.regenerating_shot=None;job.pending_revision_job_id=None;
  job.revision_error=None;
 }).await?;
 Ok(())
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
  bad=input;bad.visual_engine="comfyui".into();bad.duration=91;
  assert!(validate(&bad).is_err());
 }
 #[test]
 fn AI_storyboard_gets_exact_duration_from_orchestrator(){
  let raw=json!({"title":"City","description":"A story","tags":["design"],
   "scenes":[{"heading":"Start","narration":"One","visual_prompt":"An avenue"},
    {"heading":"End","narration":"Two","visual_prompt":"A skyline"}]}).to_string();
  let (_,_,_,shots,_)=storyboard(&raw,13,false).unwrap();
  assert_eq!(shots.iter().map(|s|s.seconds).sum::<u32>(),13);
  let (_,_,_,comfy,_)=storyboard(&raw,8,true).unwrap();
   assert_eq!(comfy.iter().map(|s|s.seconds).sum::<u32>(),8);
   assert!(storyboard(&raw,3,true).is_err());
 }
  #[test]
 fn multi_shot_comfy_has_exact_duration_and_bounded_scenes(){
  let mut request:NewProduction=serde_json::from_value(json!({
   "topic":"cinematic video","visualEngine":"comfyui","duration":90
  })).unwrap();
  validate(&request).unwrap();request.duration=91;assert!(validate(&request).is_err());
  let raw=json!({"title":"Multi-shot","description":"cinema","scenes":
   (0..12).map(|i|json!({"heading":format!("Scene {i}"),
    "narration":"hello","visual_prompt":"camera shot"})).collect::<Vec<_>>()
  }).to_string();
  let (_,_,_,shots,_)=storyboard(&raw,90,true).unwrap();
  assert_eq!(shots.len(),12);
  assert_eq!(shots.iter().map(|s|s.seconds).sum::<u32>(),90);
  assert!(shots.iter().all(|s|(3..=9).contains(&s.seconds)));
 }
 #[test]
 fn visual_bible_is_structured_and_bounded(){
  let raw=json!({"title":"Test","description":"ok","visual_bible":"same red jacket, dusk lighting",
   "scenes":[{"heading":"A","narration":"a","visual_prompt":"walking"}]}).to_string();
  let (_,_,_,_,bible)=storyboard(&raw,6,true).unwrap();
  let prompt=continuity_prompt(&bible,"walking");
  assert!(prompt.contains("same red jacket"));
  assert!(prompt.contains("Scene: walking"));
  let invalid=json!({"title":"Test","description":"ok",
   "visual_bible":"A".repeat(901),"scenes":[{"heading":"A","narration":"a"}]}).to_string();
  assert!(storyboard(&invalid,6,true).is_err());
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
   tags:vec![],script:String::new(),shots:vec![],
   visual_bible:String::new(),script_attempts:0,video_id:Some(Uuid::new_v4()),
   upload_id:Some(Uuid::new_v4()),upload_url:None,
   rendered_shot_ids:Vec::new(),completed_shots:0,artifact_revision_id:None,
   revision_count:0,regenerating_shot:None,pending_revision_job_id:None,
   revision_error:None,artifact_bytes:None,
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


#[cfg(test)]
mod production_e2e_tests {
 use super::*;
 use axum::routing::post;


 #[tokio::test]
 async fn real_ffmpeg_multishot_assembly_produces_verified_mp4(){
  let available=Command::new("ffmpeg").arg("-encoders").output().await.ok()
   .is_some_and(|out|out.status.success()&&
    String::from_utf8_lossy(&out.stdout).contains("libx264"));
  let probe=Command::new("ffprobe").arg("-version").output().await.ok()
   .is_some_and(|out|out.status.success());
  if !available||!probe{
   assert_ne!(std::env::var("DOMINION_REQUIRE_VIDEO_E2E").ok().as_deref(),
    Some("1"),"FFmpeg and ffprobe required for multi-shot assembly CI");
   return;
  }
  let root=std::env::temp_dir().join(format!("multi-shot-assembly-{}",Uuid::new_v4()));
  tokio::fs::create_dir_all(&root).await.unwrap();
  let mut settings=Settings::from_env().unwrap();settings.data_dir=root.clone();
  let mut clips=Vec::new();
  for (index,filter) in [
   "color=c=blue:s=320x180:r=16","color=c=red:s=240x320:r=18"
  ].iter().enumerate(){
   let file=root.join(format!("scene-{index}.mp4"));
   let output=Command::new("ffmpeg")
    .args(["-hide_banner","-loglevel","error","-f","lavfi","-i",filter,
     "-t","3","-c:v","libx264","-pix_fmt","yuv420p","-y"])
    .arg(&file).output().await.unwrap();
   assert!(output.status.success(),"source scene generation: {}",
    String::from_utf8_lossy(&output.stderr));
   clips.push((file,3));
  }
  let id=Uuid::new_v4();
  assemble_clips(&settings,id,&clips,"9:16").await.unwrap();
  let path=assembled_path(&settings,id);
  let (size,seconds)=inspect(&path,6).await.unwrap();
  assert!(size>1024);
  assert!((5.7..=6.3).contains(&seconds),"assembled duration: {seconds}");
  tokio::fs::remove_dir_all(root).await.unwrap();
 }
 #[tokio::test]
 async fn topic_to_storyboard_to_physical_video_and_ffprobe_qa_without_upload() {
  let available=Command::new("ffmpeg").arg("-filters").output().await.ok()
   .is_some_and(|out|out.status.success()&&
    String::from_utf8_lossy(&out.stdout).contains("drawtext"));
  let ffprobe=Command::new("ffprobe").arg("-version").output().await.ok()
   .is_some_and(|out|out.status.success());
  if !available||!ffprobe{
   assert_ne!(std::env::var("DOMINION_REQUIRE_VIDEO_E2E").ok().as_deref(),
    Some("1"),"CI requires FFmpeg with drawtext and ffprobe for actual video rendering");
   return;
  }
  let root=std::env::temp_dir().join(format!("video-production-e2e-{}",Uuid::new_v4()));
  let story=json!({"title":"A city evening","description":"A real rendered city story",
   "tags":["city","cinema"],
   "scenes":[
    {"heading":"First light","narration":"The city wakes.","visual_prompt":"A sunlit street"},
    {"heading":"The skyline","narration":"The skyline glows.","visual_prompt":"A city skyline"}
   ]}).to_string();
  let app=axum::Router::new().route("/chat/completions",post(move||{
   let response=story.clone();
   async move{
    Json(json!({"choices":[{"message":{"role":"assistant","content":response}}],
     "usage":{"prompt_tokens":20,"completion_tokens":80}}))
   }
  }));
  let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let addr=listener.local_addr().unwrap();
  let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap();});
  let mut settings=Settings::from_env().unwrap();
  settings.data_dir=root.join("data");
  settings.workspace_dir=root.join("workspace");
  settings.main.base_url=format!("http://{addr}");
  settings.main.api_key.clear();
  settings.economy=settings.main.clone();
  let sdk=crate::sdk::Dominion::open(settings.clone()).await.unwrap();
  let request=NewProduction{
   topic:"A city at sunset".into(),audience:"architecture lovers".into(),
   source_notes:"A fictional cinematic montage".into(),
   visual_engine:"native".into(),aspect_ratio:"1:1".into(),
   duration:6,narration:false,auto_publish:false,
   privacy_status:"private".into(),max_cost_krw:3000.0,
  };
  let (code,Json(created))=create(State(sdk.engine()),Json(request)).await.unwrap();
  assert_eq!(code,StatusCode::ACCEPTED);
  let id=Uuid::parse_str(created["id"].as_str().unwrap()).unwrap();
  let done=tokio::time::timeout(Duration::from_secs(90),async{
   loop{
    tokio::time::sleep(Duration::from_millis(250)).await;
    let current=read(&settings).unwrap().jobs.remove(&id).unwrap();
    if current.status!="running"{return current;}
   }
  }).await.expect("video production did not settle in 90 seconds");
  assert_eq!(done.status,"ready","production failed: {:?}",done.error);
  assert_eq!(done.shots.len(),2);
  assert_eq!(done.shots.iter().map(|s|s.seconds).sum::<u32>(),6);
  assert!(done.artifact_bytes.unwrap_or_default()>1024);
  assert!(done.measured_seconds.unwrap_or_default()>4.0);
  assert!(done.video_id.is_some());
  assert!(done.upload_id.is_none(),"upload must stay opt-in");
  let video=sdk.engine().videos.output_path(done.video_id.unwrap()).await.unwrap();
  assert!(tokio::fs::metadata(video).await.unwrap().len()>1024);
  server.abort();
  drop(sdk);
  tokio::fs::remove_dir_all(root).await.unwrap();
 }
}
