//! Main Video UI uses native FFmpeg and opt-in local ComfyUI API-format workflows.
//! Jobs and actual artifacts are backed by their respective Rust stores.
use crate::{
    engine::Engine,
    video::{NewVideo,Scene,Video,VideoBackend,VideoStatus},
};
use axum::{
    body::Body,extract::{Path,State},http::{header,StatusCode},
    response::{IntoResponse,Response},routing::{get,post},Json,Router,
};
use serde::Deserialize;
use serde_json::{json,Value};
use std::{sync::Arc,time::Duration};
use tokio_util::io::ReaderStream;
use uuid::Uuid;

type Fault=(StatusCode,Json<Value>);
fn error(status:StatusCode,msg:impl Into<String>)->Fault{
    (status,Json(json!({"error":msg.into()})))
}
fn bad(msg:impl Into<String>)->Fault{error(StatusCode::BAD_REQUEST,msg)}
fn not_found()->Fault{error(StatusCode::NOT_FOUND,"video job not found")}
fn unsupported(msg:&str)->Fault{error(StatusCode::NOT_IMPLEMENTED,msg)}
pub fn routes()->Router<Arc<Engine>>{
    Router::new()
        .route("/video/status",get(status))
        .route("/video/config",get(config).post(update_config))
        .route("/video/catalog",get(catalog))
        .route("/video/outputs",get(outputs))
        .route("/video/workflows",get(workflows))
        .route("/video/models/downloads",get(downloads))
        .route("/video/jobs",get(jobs).post(create_job))
        .route("/video/jobs/{id}",get(job))
        .route("/video/jobs/{id}/content",get(content))
        .route("/video/jobs/{id}/reveal",post(reveal))
        // Actual OAuth-backed YouTube upload jobs live in frontend_youtube.
        // OAuth provider config and login are owned by frontend_oauth.
}
async fn ffmpeg_available()->bool{
    let probe=tokio::process::Command::new("ffmpeg")
        .arg("-filters").kill_on_drop(true).output();
    tokio::time::timeout(Duration::from_secs(3),probe).await
        .ok().and_then(Result::ok)
        .is_some_and(|out|out.status.success() &&
            String::from_utf8_lossy(&out.stdout).contains("drawtext"))
}
async fn speech_available()->bool{
    if std::env::var("DOMINION_DISABLE_TTS").ok().as_deref()==Some("true"){return false;}
    tokio::time::timeout(Duration::from_secs(2),tokio::process::Command::new("espeak")
        .arg("--version").kill_on_drop(true).output()).await.ok()
        .and_then(Result::ok).is_some_and(|result|result.status.success())
}
async fn status(State(engine):State<Arc<Engine>>)->Result<Json<Value>,Fault>{
    let jobs=engine.videos.list().await;
    let mut list=Vec::with_capacity(jobs.len());
    for v in &jobs{list.push(as_job(&engine,v).await);}
    let cfg=crate::frontend_comfy::config(&engine.settings).map_err(bad)?;
    let reachable=crate::frontend_comfy::reachable(&cfg).await;
    let uses_gguf=cfg.video_model.starts_with("unet/");
    let gguf_ready=if uses_gguf&&reachable{
        crate::frontend_comfy::gguf_ready(&cfg).await
    }else{!uses_gguf};
    list.extend(crate::frontend_comfy::list(&engine.settings).map_err(bad)?);
    list.sort_by(|a,b|b["createdAt"].as_str().cmp(&a["createdAt"].as_str()));
    let available=ffmpeg_available().await;
    let narration=speech_available().await;
    let mut config=crate::frontend_comfy::public_config(&cfg);
    if !narration{config["narration"]=json!(false);}
    Ok(Json(json!({
        "config":config,
        "native":{"available":available},
        "comfy":{"reachable":reachable,"available":reachable&&cfg.comfy_workflow.is_some()&&gguf_ready,
            "ggufReady":gguf_ready,"requiresGguf":uses_gguf},
        "workflows":crate::frontend_comfy::workflows(&cfg),"jobs":list
    })))
}
async fn config(State(engine):State<Arc<Engine>>)->Result<Json<Value>,Fault>{
    crate::frontend_comfy::config(&engine.settings)
        .map(|cfg|Json(crate::frontend_comfy::public_config(&cfg))).map_err(bad)
}
async fn update_config(State(engine):State<Arc<Engine>>,Json(input):Json<Value>)
    ->Result<Json<Value>,Fault>{
    crate::frontend_comfy::update(&engine.settings,&input)
        .map(|cfg|Json(crate::frontend_comfy::public_config(&cfg))).map_err(bad)
}
async fn catalog(State(engine):State<Arc<Engine>>)->Json<Value>{
    let models=crate::frontend_comfy::catalog(&engine.settings).await;
    Json(json!({"videoModels":models}))
}
async fn outputs(State(engine):State<Arc<Engine>>)->Result<Json<Value>,Fault>{
    let all=engine.videos.list().await;
    let mut rows=Vec::new();
    for v in &all{
        if v.status==VideoStatus::Completed{rows.push(as_job(&engine,v).await);}
    }
    rows.extend(crate::frontend_comfy::list(&engine.settings).map_err(bad)?
        .into_iter().filter(|row|row["status"]=="completed"));
    rows.sort_by(|a,b|b["createdAt"].as_str().cmp(&a["createdAt"].as_str()));
    Ok(Json(json!(rows)))
}
async fn workflows(State(engine):State<Arc<Engine>>)->Result<Json<Value>,Fault>{
    let cfg=crate::frontend_comfy::config(&engine.settings).map_err(bad)?;
    Ok(Json(json!(crate::frontend_comfy::workflows(&cfg))))
}
async fn downloads(State(engine):State<Arc<Engine>>)->Json<Value>{
 let Json(pack)=crate::frontend_video_models::status(State(engine)).await;
 Json(json!({"downloads":[pack]}))
}
async fn jobs(State(engine):State<Arc<Engine>>)->Result<Json<Value>,Fault>{
    let all=engine.videos.list().await;
    let mut rows=Vec::new();
    for v in &all{rows.push(as_job(&engine,v).await);}
    rows.extend(crate::frontend_comfy::list(&engine.settings).map_err(bad)?);
    rows.sort_by(|a,b|b["createdAt"].as_str().cmp(&a["createdAt"].as_str()));
    Ok(Json(json!(rows)))
}
async fn as_job(engine:&Engine,video:&Video)->Value{
    let status=match video.status{
        VideoStatus::Draft=>"queued",VideoStatus::Rendering=>"running",
        VideoStatus::Completed=>"completed",VideoStatus::Failed=>"failed"
    };
    let output_url=if video.status==VideoStatus::Completed{
        Some(format!("/api/video/jobs/{}/content",video.id))
    }else{None};
    let result=if video.status==VideoStatus::Completed{
        match engine.videos.output_path(video.id).await{
            Ok(path)=>tokio::fs::metadata(path).await.ok()
                .map(|meta|json!({"bytes":meta.len()})),
            Err(_)=>None,
        }
    }else{None};
    json!({
        "id":video.id,"kind":"video","status":status,
        "title":video.title.as_deref().unwrap_or(""),
        "brief":video.prompt,"createdAt":crate::frontend_state::timestamp(video.created_at_ms),
        "updatedAt":crate::frontend_state::timestamp(video.updated_at_ms),
        "outputUrl":output_url,"result":result,"error":video.error,
        "costKrw":video.cost_krw,
        "duration":video.scenes.iter().map(|s|s.seconds).sum::<u32>(),
        "aspectRatio":video.aspect_ratio,
    })
}
async fn job(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
    ->Result<Json<Value>,Fault>{
    if let Some(v)=engine.videos.get(id).await{
        return Ok(Json(as_job(&engine,&v).await));
    }
    let result=crate::frontend_comfy::get(&engine.settings,id).map_err(bad)?
        .ok_or_else(not_found)?;
    Ok(Json(crate::frontend_comfy::public(&result)))
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct Create{
    engine:String,
    title:Option<String>,
    brief:String,
    aspect_ratio:String,
    duration:u32,
    #[serde(default)]
    negative_prompt:Option<String>,
    fps:Option<u32>,
    seed:Option<i64>,
}
async fn create_job(State(engine):State<Arc<Engine>>,Json(input):Json<Create>)
    ->Result<(StatusCode,Json<Value>),Fault>{
    if input.engine=="comfyui"{
        let request=crate::frontend_comfy::Create{
            title:input.title.as_deref().filter(|v|!v.trim().is_empty())
                .unwrap_or("ComfyUI Video").to_owned(),
            brief:input.brief.clone(),negative:input.negative_prompt.unwrap_or_default(),
            aspect_ratio:input.aspect_ratio,duration:input.duration,
            fps:input.fps.unwrap_or(16),seed:input.seed.unwrap_or(-1)
        };
        let record=crate::frontend_comfy::create(engine.settings.clone(),request)
            .await.map_err(bad)?;
        return Ok((StatusCode::ACCEPTED,Json(record)));
    }
    if input.engine!="native"{
        return Err(unsupported("unknown video engine"));
    }
    if input.negative_prompt.is_some()||input.fps.is_some()||input.seed.is_some(){
        return Err(unsupported("Model-specific generation controls cannot be applied by local FFmpeg."));
    }
    if !matches!(input.aspect_ratio.as_str(),"16:9"|"9:16"|"1:1"){
        return Err(bad("aspect ratio must be 16:9, 9:16, or 1:1"));
    }
    if !(3..=180).contains(&input.duration){
        return Err(bad("duration must be 3-180 seconds"));
    }
    let brief=input.brief.trim();
    if brief.is_empty()||brief.len()>1000{
        return Err(bad("video prompt must be 1-1000 bytes for a local narrated scene"));
    }
    let title=input.title.as_deref().filter(|s|!s.trim().is_empty()).unwrap_or("Dominion Video");
    if title.len()>180{return Err(bad("video title exceeds 180 bytes"));}
    if !ffmpeg_available().await{
        return Err(error(StatusCode::SERVICE_UNAVAILABLE,"FFmpeg with drawtext is required for native video rendering"));
    }
    // The native backend generates real FFmpeg title-card clips and narration.
    // Distribute the requested seconds exactly; no unsupported motion-generation
    // results are ever claimed.
    let parts=input.duration.div_ceil(30);
    let basic=input.duration/parts;
    let extra=input.duration%parts;
    let scenes=(0..parts).map(|i|Scene{
        heading:if parts==1{title.to_owned()}else{format!("{} · {}",title,i+1)},
        narration:brief.to_owned(),
        seconds:basic+u32::from(i<extra),
    }).collect::<Vec<_>>();
    let created=engine.videos.create(&engine.model,NewVideo{
        prompt:brief.into(),scenes:Some(scenes),backend:Some(VideoBackend::Local),
        aspect_ratio:Some(input.aspect_ratio)
    }).await.map_err(bad)?;
    let prepared=engine.videos.set_title(created.id,title.into()).await.map_err(bad)?;
    let narration=crate::frontend_comfy::config(&engine.settings).map_err(bad)?.narration;
    engine.videos.set_narration(prepared.id,narration).await.map_err(bad)?;
    let started=engine.videos.start(prepared.id).await.map_err(bad)?;
    Ok((StatusCode::ACCEPTED,Json(as_job(&engine,&started).await)))
}
async fn content(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
    ->Result<Response,Fault>{
    let (path,mime)=if engine.videos.get(id).await.is_some(){
        (engine.videos.output_path(id).await.map_err(bad)?,"video/mp4")
    }else{
        crate::frontend_comfy::output(&engine.settings,id).map_err(bad)?
            .ok_or_else(not_found)?
    };
    let file=tokio::fs::File::open(path).await
        .map_err(|e|error(StatusCode::NOT_FOUND,e.to_string()))?;
    let body=Body::from_stream(ReaderStream::new(file));
    Ok(([(header::CONTENT_TYPE,mime),
        (header::CONTENT_DISPOSITION,"inline; filename=dominion-video"),
        (header::CACHE_CONTROL,"no-store")],body).into_response())
}
async fn reveal(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
    ->Result<Json<Value>,Fault>{
    let path=if engine.videos.get(id).await.is_some(){
        engine.videos.output_path(id).await.map_err(bad)?
    }else{
        crate::frontend_comfy::output(&engine.settings,id).map_err(bad)?
            .ok_or_else(not_found)?.0
    };
    // A request made by a remote/browser client must not spawn an arbitrary
    // local viewer or return a raw server filesystem path.
    Ok(Json(json!({"ok":true,"downloadUrl":format!("/api/video/jobs/{id}/content"),
        "bytes":tokio::fs::metadata(path).await.map_err(|e|bad(e.to_string()))?.len()})))
}
async fn youtube_status()->Json<Value>{
    Json(json!({"connected":false,"uploadAllowed":false,"account":null,"uploads":[]}))
}
async fn youtube_uploads()->Json<Value>{Json(json!([]))}
async fn oauth_providers()->Json<Value>{Json(json!([]))}
