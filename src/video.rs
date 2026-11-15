use crate::{domain::now_ms, provider::ModelClient};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::{collections::{HashMap,HashSet},path::{Path,PathBuf},process::Stdio,sync::Arc,time::Duration};
use tokio::{fs,process::Command,sync::Mutex};
use uuid::Uuid;
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct Scene { pub heading:String, pub narration:String, pub seconds:u32 }
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="snake_case")]
pub enum VideoStatus { Draft, Rendering, Completed, Failed }
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq,Default)]
#[serde(rename_all="snake_case")]
pub enum VideoBackend { #[default] Local, Veo }
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct Video {
    pub id:Uuid, pub prompt:String, #[serde(default)] pub title:Option<String>,
    pub scenes:Vec<Scene>, pub status:VideoStatus,
    pub cost_krw:f64, pub render_seconds:u64, pub error:Option<String>,
    pub output:Option<String>, pub created_at_ms:u64, pub updated_at_ms:u64,
    #[serde(default)] pub backend:VideoBackend,
    #[serde(default="default_aspect")] pub aspect_ratio:String,
    #[serde(default)] pub remote_operations:HashMap<usize,String>,
}
#[derive(Deserialize)]
pub struct NewVideo { pub prompt:String, pub scenes:Option<Vec<Scene>>, pub backend:Option<VideoBackend>, pub aspect_ratio:Option<String> }
fn default_aspect()->String{"16:9".into()}
#[derive(Deserialize)]
pub struct ChangeScene { pub heading:String, pub narration:String, pub seconds:u32 }
pub struct Videos {
    root:PathBuf,
    state:Mutex<HashMap<Uuid,Video>>,
    active:Mutex<HashSet<Uuid>>,
}
impl Videos {
    pub async fn open(root:PathBuf)->Result<Self,String>{
        fs::create_dir_all(&root).await.map_err(|e|e.to_string())?;
        let mut state=HashMap::new();
        let mut dir=fs::read_dir(&root).await.map_err(|e|e.to_string())?;
        while let Some(entry)=dir.next_entry().await.map_err(|e|e.to_string())?{
            if entry.path().extension().and_then(|s|s.to_str())!=Some("json"){continue;}
            let mut video:Video=serde_json::from_slice(&fs::read(entry.path()).await.map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
            if video.status==VideoStatus::Rendering {
                video.status=VideoStatus::Failed;
                video.error=Some("Render interrupted; rerun to resume from saved clips".into());
            }
            state.insert(video.id,video);
        }
        Ok(Self{root,state:Mutex::new(state),active:Mutex::new(HashSet::new())})
    }
    async fn put(&self,video:Video)->Result<Video,String>{
        let mut guard=self.state.lock().await;
        let tmp=self.root.join(format!("{}.tmp",video.id));
        let dst=self.root.join(format!("{}.json",video.id));
        fs::write(&tmp,serde_json::to_vec(&video).map_err(|e|e.to_string())?).await.map_err(|e|e.to_string())?;
        fs::rename(tmp,dst).await.map_err(|e|e.to_string())?;
        guard.insert(video.id,video.clone());Ok(video)
    }
    pub async fn get(&self,id:Uuid)->Option<Video>{self.state.lock().await.get(&id).cloned()}
    pub async fn list(&self)->Vec<Video>{
        let mut all:Vec<_>=self.state.lock().await.values().cloned().collect();
        all.sort_by_key(|v|std::cmp::Reverse(v.created_at_ms));all
    }
    pub async fn create(&self,model:&ModelClient,input:NewVideo)->Result<Video,String>{
        let prompt=input.prompt.trim();
        if prompt.is_empty()||prompt.len()>10_000{return Err("video prompt must be 1..10000 characters".into());}
        let backend=input.backend.unwrap_or_default();
        let aspect_ratio=input.aspect_ratio.unwrap_or_else(default_aspect);
        if !matches!(aspect_ratio.as_str(),"16:9"|"9:16"|"1:1"){
            return Err("unsupported video aspect ratio".into());
        }
        if backend==VideoBackend::Veo { let _=crate::veo::Veo::from_env()?; }
        let (scenes,cost)=match input.scenes {
            Some(items)=>(items,0.0),
            None=>{
                let requests=vec![
                    json!({"role":"system","content":format!("You write production-ready short-form video storyboards. Output only JSON: {{\"scenes\":[{{\"heading\":\"...\",\"narration\":\"...\",\"seconds\":4}}]}}. Provide 3..6 coherent scenes, concise narration, a satisfying ending, in the requested language. {}",
                        if backend==VideoBackend::Veo {"Every scene MUST have duration 4, 6 or 8 seconds."}else{"Each scene should be 4..10 seconds."})}),
                    json!({"role":"user","content":prompt}),
                ];
                let answer=model.complete(&requests,&[],true).await?;
                let raw=answer.text.trim();
                let text=raw.strip_prefix("```json").or_else(||raw.strip_prefix("```")).unwrap_or(raw).trim_end_matches("```").trim();
                let doc:Value=serde_json::from_str(text).map_err(|e|format!("storyboard JSON invalid: {e}"))?;
                let parsed=serde_json::from_value(doc.get("scenes").cloned().ok_or("storyboard has no scenes")?).map_err(|e|format!("storyboard scene invalid: {e}"))?;
                (parsed,answer.cost_krw)
            },
        };
        validate_scenes(&scenes)?;
        if backend==VideoBackend::Veo && scenes.iter().any(|scene|!matches!(scene.seconds,4|6|8)) {
            return Err("Veo scenes must be 4, 6 or 8 seconds".into());
        }
        let now=now_ms();
        let video=Video{id:Uuid::new_v4(),prompt:prompt.into(),title:None,scenes,status:VideoStatus::Draft,cost_krw:cost,render_seconds:0,error:None,output:None,created_at_ms:now,updated_at_ms:now,backend,aspect_ratio,remote_operations:HashMap::new()};
        self.put(video).await
    }
    /// Display name is stored with the actual rendered job, never as a
    /// parallel phantom record in the compatibility adapter.
    pub async fn set_title(&self,id:Uuid,title:String)->Result<Video,String>{
        if title.len()>180{return Err("video title too long".into());}
        let mut item=self.get(id).await.ok_or("video not found")?;
        if item.status!=VideoStatus::Draft{return Err("title must be set before rendering".into());}
        item.title=Some(title);
        item.updated_at_ms=now_ms();
        self.put(item).await
    }
    pub async fn change_scene(&self,id:Uuid,index:usize,scene:ChangeScene)->Result<Video,String>{
        if self.active.lock().await.contains(&id){return Err("render in progress".into());}
        let mut video=self.get(id).await.ok_or("video not found")?;
        if index>=video.scenes.len(){return Err("scene out of range".into());}
        let replacement=Scene{heading:scene.heading,narration:scene.narration,seconds:scene.seconds};
        validate_scenes(std::slice::from_ref(&replacement))?;
        if video.backend==VideoBackend::Veo && !matches!(replacement.seconds,4|6|8) {
            return Err("Veo scene must be 4, 6 or 8 seconds".into());
        }
        video.remote_operations.remove(&index);
        video.scenes[index]=replacement;
        video.status=VideoStatus::Draft;video.error=None;video.output=None;video.updated_at_ms=now_ms();
        let clip=self.root.join(id.to_string()).join(format!("scene-{index}.mp4"));
        let _=fs::remove_file(clip).await; // preserve unchanged scenes for partial regeneration
        self.put(video).await
    }
    pub async fn start(self:&Arc<Self>,id:Uuid)->Result<Video,String>{
        {let mut lock=self.active.lock().await;if !lock.insert(id){return Err("render already active".into());}}
        let mut video=match self.get(id).await {
            Some(v)=>v,
            None=>{self.active.lock().await.remove(&id);return Err("video not found".into());}
        };
        video.status=VideoStatus::Rendering;
        video.error=None;video.output=None;video.updated_at_ms=now_ms();
        if let Err(e)=self.put(video.clone()).await {self.active.lock().await.remove(&id);return Err(e);}
        let videos=Arc::clone(self);
        tokio::spawn(async move{
            let begin=std::time::Instant::now();
            let result=videos.render(id).await;
            if let Some(mut video)=videos.get(id).await{
                video.render_seconds=video.render_seconds.saturating_add(begin.elapsed().as_secs());
                video.updated_at_ms=now_ms();
                match result {
                    Ok(file)=>{video.status=VideoStatus::Completed;video.output=Some(file);video.error=None;}
                    Err(error)=>{video.status=VideoStatus::Failed;video.output=None;video.error=Some(error);}
                }
                if let Err(error)=videos.put(video).await{tracing::error!("video persist failed: {error}");}
            }
            videos.active.lock().await.remove(&id);
        });
        Ok(video)
    }
    async fn render(&self,id:Uuid)->Result<String,String>{
        let video=self.get(id).await.ok_or("video not found")?;
        let directory=self.root.join(id.to_string());
        fs::create_dir_all(&directory).await.map_err(|e|e.to_string())?;
        if video.backend==VideoBackend::Veo {
            return self.render_veo(id,&video,&directory).await;
        }
        for (index,scene) in video.scenes.iter().enumerate(){
            let clip=directory.join(format!("scene-{index}.mp4"));
            if fs::metadata(&clip).await.is_ok(){continue;}
            let textfile=directory.join(format!("scene-{index}.txt"));
            let text=format!("{}\n\n{}",scene.heading,scene.narration);
            fs::write(&textfile,&text).await.map_err(|e|e.to_string())?;
            let voice=directory.join(format!("scene-{index}.wav"));
            let speech=make_speech(&directory,&scene.narration,&voice).await;
            let color=["0x16263f","0x202a48","0x27384b","0x172c3c","0x2d2948","0x24414a"][index%6];
            let filter=format!("drawtext=textfile=scene-{index}.txt:fontcolor=white:fontsize=47:line_spacing=18:x=(w-text_w)/2:y=(h-text_h)/2:box=1:boxcolor=black@0.50:boxborderw=26");
            let (width,height)=match video.aspect_ratio.as_str(){
                "9:16"=>(720,1280),"1:1"=>(960,960),_=>(1280,720)
            };
            let mut command=Command::new("ffmpeg");
            command.current_dir(&directory).arg("-y").arg("-hide_banner").arg("-loglevel").arg("error")
                .arg("-f").arg("lavfi").arg("-i").arg(format!("color=c={color}:s={width}x{height}:r=24:d={}",scene.seconds));
            if speech {
                command.arg("-i").arg(&voice).arg("-af").arg("apad");
            }else{
                command.arg("-f").arg("lavfi").arg("-i").arg("anullsrc=channel_layout=stereo:sample_rate=44100");
            }
            command.arg("-vf").arg(&filter).arg("-c:v").arg("libx264").arg("-preset").arg("veryfast")
                .arg("-crf").arg("24").arg("-pix_fmt").arg("yuv420p").arg("-c:a").arg("aac")
                .arg("-b:a").arg("112k").arg("-t").arg(scene.seconds.to_string())
                .arg("-shortest").arg(&clip);
            invoke(command,Duration::from_secs(120)).await?;
        }
        self.merge_clips(id,video.scenes.len(),&directory).await
    }
    async fn render_veo(&self,id:Uuid,video:&Video,directory:&Path)->Result<String,String>{
        let veo=crate::veo::Veo::from_env()?;
        for (index,scene) in video.scenes.iter().enumerate(){
            let clip=directory.join(format!("scene-{index}.mp4"));
            if fs::metadata(&clip).await.is_ok(){continue;}
            let mut latest=self.get(id).await.ok_or("video not found")?;
            let operation=if let Some(id)=latest.remote_operations.get(&index){id.clone()}else{
                let prompt=format!("{}\n\nVisual direction: {}\n\nDialogue and narration: {}",
                    video.prompt,scene.heading,scene.narration);
                let operation=veo.start(&prompt,scene.seconds).await?;
                latest.remote_operations.insert(index,operation.clone());
                latest.cost_krw+=veo.estimate_krw(scene.seconds);
                latest.updated_at_ms=now_ms();
                self.put(latest).await?;
                operation
            };
            let raw=directory.join(format!("scene-{index}.original.mp4"));
            if fs::metadata(&raw).await.is_err(){
                let url=veo.wait(&operation).await?;
                veo.download(&url,&raw).await?;
            }
            let (width,height)=match video.aspect_ratio.as_str(){
                "9:16"=>(720,1280),"1:1"=>(960,960),_=>(1280,720)
            };
            let mut command=Command::new("ffmpeg");
            command.current_dir(directory).arg("-y").arg("-hide_banner").arg("-loglevel").arg("error")
                .arg("-i").arg(&raw).arg("-map").arg("0:v:0").arg("-map").arg("0:a:0?")
                .arg("-vf").arg(format!("scale={width}:{height}:force_original_aspect_ratio=decrease,pad={width}:{height}:(ow-iw)/2:(oh-ih)/2,fps=24"))
                .arg("-c:v").arg("libx264").arg("-preset").arg("veryfast")
                .arg("-crf").arg("24").arg("-pix_fmt").arg("yuv420p")
                .arg("-c:a").arg("aac").arg("-ar").arg("44100").arg("-ac").arg("2")
                .arg("-t").arg(scene.seconds.to_string()).arg(&clip);
            invoke(command,Duration::from_secs(150)).await?;
        }
        self.merge_clips(id,video.scenes.len(),directory).await
    }
    async fn merge_clips(&self,id:Uuid,count:usize,directory:&Path)->Result<String,String>{
        let manifest=(0..count).map(|i|format!("file 'scene-{i}.mp4'")).collect::<Vec<_>>().join("\n");
        fs::write(directory.join("concat.txt"),format!("{manifest}\n")).await.map_err(|e|e.to_string())?;
        let tmp=directory.join("rendering.mp4");
        let final_name=directory.join("finished.mp4");
        let mut merge=Command::new("ffmpeg");
        merge.current_dir(directory).arg("-y").arg("-hide_banner").arg("-loglevel").arg("error")
            .arg("-f").arg("concat").arg("-safe").arg("0").arg("-i").arg("concat.txt")
            .arg("-c").arg("copy").arg(&tmp);
        invoke(merge,Duration::from_secs(120)).await?;
        fs::rename(&tmp,&final_name).await.map_err(|e|e.to_string())?;
        Ok(format!("{id}/finished.mp4"))
    }
    pub async fn output_path(&self,id:Uuid)->Result<PathBuf,String>{
        let video=self.get(id).await.ok_or("video not found")?;
        if video.status!=VideoStatus::Completed{return Err("video not completed".into());}
        let path=video.output.ok_or("no rendered video")?;
        Ok(self.root.join(path))
    }
}
fn validate_scenes(scenes:&[Scene])->Result<(),String>{
    if scenes.is_empty()||scenes.len()>12{return Err("video needs 1..12 scenes".into());}
    let mut total=0u32;
    for scene in scenes {
        if scene.heading.trim().is_empty()||scene.heading.len()>180||scene.narration.len()>1000||!(2..=30).contains(&scene.seconds) {
            return Err("invalid scene heading, narration or duration".into());
        }
        total=total.saturating_add(scene.seconds);
    }
    if total>180 {return Err("video duration exceeds 180 seconds".into());}
    Ok(())
}
async fn make_speech(dir:&Path,text:&str,voice:&Path)->bool{
    if std::env::var("DOMINION_DISABLE_TTS").ok().as_deref()==Some("true"){return false;}
    let mut command=Command::new("espeak");
    command.current_dir(dir).arg("-s").arg("155").arg("-w").arg(voice).arg(text);
    invoke(command,Duration::from_secs(20)).await.is_ok()
}
async fn invoke(mut command:Command,deadline:Duration)->Result<(),String>{
    command.stdout(Stdio::null()).stderr(Stdio::piped()).kill_on_drop(true);
    let output=tokio::time::timeout(deadline,command.output()).await.map_err(|_|"render subprocess timeout".to_string())?.map_err(|e|e.to_string())?;
    if output.status.success(){Ok(())}else{
        Err(format!("render subprocess failed: {}",String::from_utf8_lossy(&output.stderr).chars().take(1600).collect::<String>()))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn caps_video_duration(){
        let scenes=(0..7).map(|_|Scene{heading:"Scene".into(),narration:"Voice".into(),seconds:30}).collect::<Vec<_>>();
        assert!(validate_scenes(&scenes).is_err());
    }
    #[tokio::test]
    async fn persists_scene_revisions(){
        let dir=std::env::temp_dir().join(format!("dominion-video-test-{}",Uuid::new_v4()));
        let store=Videos::open(dir.clone()).await.unwrap();
        let video=Video{id:Uuid::new_v4(),prompt:"demo".into(),title:None,scenes:vec![Scene{heading:"old".into(),narration:"old".into(),seconds:4}],status:VideoStatus::Draft,cost_krw:0.0,render_seconds:0,error:None,output:None,created_at_ms:now_ms(),updated_at_ms:now_ms(),backend:VideoBackend::Local,aspect_ratio:default_aspect(),remote_operations:HashMap::new()};
        let id=video.id;store.put(video).await.unwrap();
        store.change_scene(id,0,ChangeScene{heading:"new".into(),narration:"text".into(),seconds:4}).await.unwrap();
        assert_eq!(store.get(id).await.unwrap().scenes[0].heading,"new");
        fs::remove_dir_all(dir).await.unwrap();
    }
}

#[cfg(test)]
mod production_tests {
    use super::*;
    use crate::config::Settings;
    #[tokio::test]
    async fn renders_real_mp4_from_manual_scenes() {
        if Command::new("ffmpeg").arg("-version").output().await.is_err() { return; }
        let dir=std::env::temp_dir().join(format!("dominion-video-production-{}",Uuid::new_v4()));
        let settings=Settings::from_env().unwrap();
        let model=ModelClient::new(&settings).unwrap();
        let videos=Arc::new(Videos::open(dir.clone()).await.unwrap());
        let video=videos.create(&model,NewVideo{
            prompt:"A short smoke test".into(),
            scenes:Some(vec![Scene{heading:"Runtime".into(),narration:"The local renderer produces an artifact.".into(),seconds:2}]),backend:None,aspect_ratio:None
        }).await.unwrap();
        let id=video.id;
        videos.start(id).await.unwrap();
        let mut result=None;
        for _ in 0..150 {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let snapshot=videos.get(id).await.unwrap();
            if snapshot.status!=VideoStatus::Rendering {result=Some(snapshot);break;}
        }
        let result=result.expect("ffmpeg render timed out");
        assert_eq!(result.status,VideoStatus::Completed,"render error: {:?}",result.error);
        let file=videos.output_path(id).await.unwrap();
        assert!(fs::metadata(&file).await.unwrap().len()>1000);
        fs::remove_dir_all(dir).await.unwrap();
    }
}
