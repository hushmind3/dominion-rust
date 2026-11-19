//! Persistent user-initiated Hugging Face GGUF/MLX downloads for the main UI.
//! The native Rust Engine remains the sole agent runner. Model files are data,
//! and GGUF activation is explicitly delegated to the local Ollama runtime.
use crate::{config::Settings,domain::now_ms,engine::Engine};
use axum::{extract::{Path,State},http::StatusCode,routing::{get,post},
    Json,Router};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::{collections::HashMap,fs,io::Write,path::{Path as FilePath,PathBuf},
    sync::{Arc,OnceLock}};
use tokio::{fs as async_fs,io::AsyncWriteExt,sync::Mutex};
use uuid::Uuid;

type Fault=(StatusCode,Json<Value>);
fn fault(code:StatusCode,error:impl Into<String>)->Fault{
    (code,Json(json!({"error":error.into()})))
}
fn bad(error:impl Into<String>)->Fault{fault(StatusCode::BAD_REQUEST,error)}
fn conflict(error:impl Into<String>)->Fault{fault(StatusCode::CONFLICT,error)}
const MAX_JOBS:usize=80;
const MAX_FILES:usize=1500;
const MAX_SNAPSHOT:usize=2_000_000;
const MAX_BYTES:u64=512*1024*1024*1024;
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
struct Job{
    id:String,repo:String,option_id:String,revision:String,format:String,
    status:String,downloaded:u64,total:u64,current:String,error:String,
    installed_id:Option<String>,started_at:String,files:Vec<String>,
    #[serde(default)]generation:u64,
    #[serde(default)]sizes:Vec<u64>,
}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub(crate) struct Installed{
    id:String,repo:String,option_id:String,revision:String,source:String,
    format:String,label:String,bytes:u64,installed_at:String,
    external_path:Option<String>,files:Vec<String>,
    #[serde(default)]ollama_model:Option<String>,
}
impl Installed{
    fn public(&self)->Value{json!({
        "id":self.id,"repo":self.repo,"optionId":self.option_id,
        "source":self.source,"format":self.format,"label":self.label,
        "bytes":self.bytes,"installedAt":self.installed_at,
        "externalPath":self.external_path
    })}
}
#[derive(Default,Serialize,Deserialize)]
struct Snapshot{jobs:HashMap<String,Job>,installed:HashMap<String,Installed>}
type Shared=Arc<Mutex<Snapshot>>;
static MANAGERS:OnceLock<Mutex<HashMap<PathBuf,Shared>>>=OnceLock::new();
static INFLIGHT:OnceLock<Mutex<HashMap<String,Arc<Mutex<()>>>>>=OnceLock::new();
async fn job_gate(id:&str)->Arc<Mutex<()>>{
    let global=INFLIGHT.get_or_init(||Mutex::new(HashMap::new()));
    let mut entries=global.lock().await;
    if entries.len()>128{entries.retain(|_,gate|Arc::strong_count(gate)>1);}
    Arc::clone(entries.entry(id.to_owned())
        .or_insert_with(||Arc::new(Mutex::new(()))))
}
fn base(settings:&Settings)->PathBuf{settings.data_dir.join("model-library")}
fn snapshot_file(root:&FilePath)->PathBuf{root.join("index.json")}
fn persist(root:&FilePath,state:&Snapshot)->Result<(),String>{
    let bytes=serde_json::to_vec(state).map_err(|e|e.to_string())?;
    if bytes.len()>MAX_SNAPSHOT{return Err("model library index exceeds 2 MB".into());}
    fs::create_dir_all(root).map_err(|e|e.to_string())?;
    let target=snapshot_file(root);
    let tmp=root.join(format!(".model-index-{}",Uuid::new_v4()));
    let result=(||->Result<(),String>{
        let mut op=fs::OpenOptions::new();op.write(true).create_new(true);
        #[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;op.mode(0o600);}
        let mut file=op.open(&tmp).map_err(|e|e.to_string())?;
        file.write_all(&bytes).map_err(|e|e.to_string())?;
        file.sync_all().map_err(|e|e.to_string())?;
        drop(file);
        fs::rename(&tmp,target).map_err(|e|e.to_string())?;
        #[cfg(unix)]fs::File::open(root).and_then(|f|f.sync_all()).map_err(|e|e.to_string())?;
        Ok(())
    })();
    if result.is_err(){let _=fs::remove_file(tmp);}
    result
}
async fn state(settings:&Settings)->Result<(PathBuf,Shared),String>{
    let root=base(settings);
    let registry=MANAGERS.get_or_init(||Mutex::new(HashMap::new()));
    let mut managers=registry.lock().await;
    if let Some(existing)=managers.get(&root){return Ok((root,Arc::clone(existing)));}
    fs::create_dir_all(&root).map_err(|e|e.to_string())?;
    let mut snapshot=if snapshot_file(&root).is_file(){
        let bytes=fs::read(snapshot_file(&root)).map_err(|e|e.to_string())?;
        if bytes.len()>MAX_SNAPSHOT{return Err("model index too large".into());}
        serde_json::from_slice::<Snapshot>(&bytes)
            .map_err(|e|format!("invalid model index: {e}"))?
    }else{Snapshot::default()};
    let mut changed=false;
    for job in snapshot.jobs.values_mut(){
        if matches!(job.status.as_str(),"downloading"|"resolving"){
            job.status="paused".into();changed=true;
        }
    }
    if changed{persist(&root,&snapshot)?;}
    let shared=Arc::new(Mutex::new(snapshot));
    managers.insert(root.clone(),Arc::clone(&shared));
    Ok((root,shared))
}
fn valid_repo(repo:&str)->bool{
    let mut split=repo.split('/');
    match (split.next(),split.next(),split.next()){
        (Some(a),Some(b),None)=>[a,b].into_iter().all(|name|!name.is_empty()&&name.len()<100
            &&name!="."&&name!=".."&&name.bytes().all(|ch|
                ch.is_ascii_alphanumeric()||matches!(ch,b'-'|b'_'|b'.'))),
        _=>false
    }
}
fn valid_file(path:&str)->bool{
    !path.is_empty()&&path.len()<512&&!path.contains('\\')
        &&!path.chars().any(char::is_control)&&!path.starts_with('/')
        &&path.split('/').all(|v|!v.is_empty()&&v!="."&&v!="..")
}
fn valid_revision(revision:&str)->bool{
    revision.len()==40&&revision.bytes().all(|c|c.is_ascii_hexdigit())
}
fn job_dir(root:&FilePath,id:&str)->Result<PathBuf,String>{
    Uuid::parse_str(id).map_err(|_|"invalid model job ID")?;
    Ok(root.join("files").join(id))
}
fn stopped(s:&Snapshot,id:&str,generation:u64)->bool{
    s.jobs.get(id).is_none_or(|j|
        j.status!="downloading"||j.generation!=generation)
}
fn disk_progress(root:&FilePath,job:&Job)->u64{
    let Ok(dir)=job_dir(root,&job.id)else{return 0;};
    job.files.iter().enumerate().map(|(i,file)|{
        let path=dir.join(file);
        let part=path.with_extension(format!("{}part",path.extension()
            .and_then(|e|e.to_str()).map_or(String::new(),|s|format!("{s}."))));
        let bytes=fs::metadata(&path).or_else(|_|fs::metadata(&part))
            .map(|m|m.len()).unwrap_or(0);
        let size=job.sizes.get(i).copied().unwrap_or(0);
        if size>0{bytes.min(size)}else{bytes}
    }).sum::<u64>().min(job.total)
}
async fn update_job(root:&FilePath,shared:&Shared,id:&str,generation:u64,
    f:impl FnOnce(&mut Job))->Result<bool,String>{
    let mut snap=shared.lock().await;
    let Some(job)=snap.jobs.get_mut(id)else{return Ok(false)};
    if job.status!="downloading"||job.generation!=generation{return Ok(false);}
    f(job);
    persist(root,&snap)?;
    Ok(true)
}
async fn job_failed(root:&FilePath,shared:&Shared,id:&str,generation:u64,reason:String){
    let _=update_job(root,shared,id,generation,|j|{
        j.status="failed".into();
        j.error=reason.chars().take(500).collect();
    }).await;
}
async fn download_file(root:&FilePath,shared:&Shared,id:&str,generation:u64,
    repo:&str,revision:&str,filename:&str,expected:u64)->Result<(),String>{
    let dest=job_dir(root,id)?.join(filename);
    let part=dest.with_extension(format!("{}part",dest.extension()
        .and_then(|e|e.to_str()).map_or(String::new(),|s|format!("{s}."))));
    if let Some(parent)=dest.parent(){
        async_fs::create_dir_all(parent).await.map_err(|e|e.to_string())?;
    }
    if dest.is_file(){
        let existing=async_fs::metadata(&dest).await.map_err(|e|e.to_string())?;
        if expected==0||existing.len()==expected{return Ok(());}
        return Err("existing Hub model file has the wrong length".into());
    }
    let offset=async_fs::metadata(&part).await.map(|m|m.len()).unwrap_or(0);
    // Encode path segments instead of treating Hub filenames as URL syntax.
    let mut url=reqwest::Url::parse("https://huggingface.co")
        .map_err(|e|e.to_string())?;
    {
        let mut segments=url.path_segments_mut()
            .map_err(|_|"Hub URL has no path segments")?;
        for part in repo.split('/').chain(["resolve",revision])
            .chain(filename.split('/')){segments.push(part);}
    }
    let client=reqwest::Client::builder().timeout(std::time::Duration::from_secs(90))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build().map_err(|e|e.to_string())?;
    let mut request=client.get(url);
    if offset>0{request=request.header(reqwest::header::RANGE,format!("bytes={offset}-"));}
    let mut response=request.send().await.map_err(|e|format!("Hugging Face transport: {e}"))?;
    if !response.status().is_success(){
        return Err(format!("Hugging Face file returned HTTP {}",response.status()));
    }
    let resumable=offset>0&&response.status()==reqwest::StatusCode::PARTIAL_CONTENT
        &&response.headers().get(reqwest::header::CONTENT_RANGE)
          .and_then(|v|v.to_str().ok())
          .is_some_and(|v|v.starts_with(&format!("bytes {offset}-")));
    if offset>0&&response.status()==reqwest::StatusCode::PARTIAL_CONTENT&&!resumable{
        return Err("Hub resumed at a different byte offset".into());
    }
    let prior=if resumable{offset}else{0};
    let mut out=async_fs::OpenOptions::new().write(true).create(true)
        .append(resumable).truncate(!resumable)
        .open(&part).await.map_err(|e|e.to_string())?;
    let mut written=prior;
    let mut checkpoint=prior;
    loop{
        if stopped(&*shared.lock().await,id,generation){return Err("download paused or cancelled".into());}
        let chunk=response.chunk().await.map_err(|e|format!("Hub stream: {e}"))?;
        let Some(chunk)=chunk else{break};
        if written.saturating_add(chunk.len() as u64)>MAX_BYTES{
            return Err("model file exceeds 512 GiB safety bound".into());
        }
        out.write_all(&chunk).await.map_err(|e|e.to_string())?;
        written+=chunk.len() as u64;
        if written.saturating_sub(checkpoint)>=16*1024*1024{
            let delta=written.saturating_sub(checkpoint);
            let ok=update_job(root,shared,id,generation,|j|{
                j.downloaded=j.downloaded.saturating_add(delta);
            }).await?;
            if !ok{return Err("download paused or cancelled".into());}
            checkpoint=written;
        }
    }
    out.sync_all().await.map_err(|e|e.to_string())?;
    drop(out);
    if stopped(&*shared.lock().await,id,generation){return Err("download paused or cancelled".into());}
    if expected>0&&written!=expected{
        return Err(format!("Hub file length differs from metadata: got {written}, expected {expected}"));
    }
    async_fs::rename(&part,&dest).await.map_err(|e|e.to_string())?;
    let final_delta=written.saturating_sub(checkpoint);
    let _=update_job(root,shared,id,generation,|j|{
        j.downloaded=j.downloaded.saturating_add(final_delta);
    }).await?;
    Ok(())
}
async fn worker(root:PathBuf,shared:Shared,id:String,generation:u64){
    // Pause+resume must not leave two writers touching one .part file.
    let gate=job_gate(&id).await;
    let _held=gate.lock().await;
    let job={shared.lock().await.jobs.get(&id).cloned()};
    let Some(job)=job else{return};
    if job.status!="downloading"||job.generation!=generation{return;}
    for (index,filename) in job.files.iter().enumerate(){
        if !update_job(&root,&shared,&id,generation,|j|j.current=filename.clone())
            .await.unwrap_or(false){return;}
        let expected=job.sizes.get(index).copied().unwrap_or(0);
        if let Err(error)=download_file(&root,&shared,&id,generation,
            &job.repo,&job.revision,filename,expected).await{
            job_failed(&root,&shared,&id,generation,error).await;return;
        }
    }
    let mut snap=shared.lock().await;
    if stopped(&snap,&id,generation){return;}
    let installed_id=format!("hf-{id}");
    let files=job.files.iter().map(|f|job_dir(&root,&id).unwrap()
        .join(f).to_string_lossy().to_string()).collect::<Vec<_>>();
    let ready=Installed{
        id:installed_id.clone(),repo:job.repo,option_id:job.option_id,
        revision:job.revision,source:"huggingface".into(),format:job.format,
        label:job.files.first().cloned().unwrap_or_default(),bytes:job.total,
        installed_at:crate::frontend_state::timestamp(now_ms()),
        external_path:None,files,ollama_model:None
    };
    snap.installed.insert(installed_id.clone(),ready);
    if let Some(entry)=snap.jobs.get_mut(&id){
        entry.status="completed".into();entry.installed_id=Some(installed_id);
        entry.current.clear();entry.downloaded=entry.total;
    }
    let _=persist(&root,&snap);
}
fn spawn(root:PathBuf,shared:Shared,id:String,generation:u64){
    tokio::spawn(worker(root,shared,id,generation));
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct DownloadRequest{
    repo:String,option_id:String,format:String,files:Vec<String>,
    revision:String,
}
async fn download(State(engine):State<Arc<Engine>>,Json(q):Json<DownloadRequest>)
    ->Result<Json<Value>,Fault>{
    if !valid_repo(&q.repo)||!valid_revision(&q.revision)
        ||!matches!(q.format.as_str(),"gguf"|"mlx")||q.files.is_empty()
        ||q.files.len()>MAX_FILES||q.files.iter().any(|f|!valid_file(f))
        ||(q.format=="gguf"&&(!q.option_id.ends_with(".gguf")
            ||q.option_id!=q.files[0]))
        ||(q.format=="mlx"&&q.option_id!="mlx"){
        return Err(bad("invalid Hugging Face model, pinned revision or file selection"));
    }
    let remote=crate::frontend_huggingface::raw_repo_details(&q.repo).await
        .map_err(|e|fault(StatusCode::BAD_GATEWAY,e))?;
    if remote["sha"].as_str()!=Some(q.revision.as_str()){
        return Err(conflict("model revision changed; refresh details before downloading"));
    }
    // The browser may specify a quantization, but may not forge a partial
    // shard group or omit required MLX config/tokenizer files.
    let parsed=crate::frontend_huggingface::detail_row(&q.repo,&remote);
    let exact=parsed["options"].as_array().into_iter().flatten().any(|option|{
        option["id"].as_str()==Some(q.option_id.as_str())
            &&option["format"].as_str()==Some(q.format.as_str())
            &&option["files"].as_array().is_some_and(|files|{
                files.iter().filter_map(Value::as_str).collect::<Vec<_>>()
                    ==q.files.iter().map(String::as_str).collect::<Vec<_>>()
            })
    });
    if !exact{return Err(bad("model files differ from the complete Hub option; refresh model details"));}
    let siblings=remote["siblings"].as_array().ok_or_else(||bad("Hub did not return file metadata"))?;
    let mut total=0u64;
    let mut sizes=Vec::new();
    for file in &q.files{
        let actual=siblings.iter().find(|v|v["rfilename"].as_str()==Some(file.as_str()))
            .ok_or_else(||bad(format!("selected Hub file is absent: {file}")))?;
        let size=actual["size"].as_u64().unwrap_or(0);
        sizes.push(size);
        total=total.saturating_add(size);
    }
    if total==0||total>MAX_BYTES{return Err(bad("missing or oversized model file sizes"));}
    let (root,shared)=state(&engine.settings).await.map_err(|e|fault(StatusCode::INTERNAL_SERVER_ERROR,e))?;
    let id=Uuid::new_v4().to_string();
    let job=Job{id:id.clone(),repo:q.repo,option_id:q.option_id,
        revision:q.revision,format:q.format,status:"downloading".into(),
        downloaded:0,total,current:String::new(),error:String::new(),
        installed_id:None,started_at:crate::frontend_state::timestamp(now_ms()),
        files:q.files,generation:0,sizes};
    {
        let mut snapshot=shared.lock().await;
        if snapshot.jobs.values().filter(|j|j.status=="downloading").count()>=2{
            return Err(conflict("at most two concurrent Hub downloads"));
        }
        if snapshot.jobs.len()>=MAX_JOBS{
            snapshot.jobs.retain(|_,j|j.status=="downloading");
            if snapshot.jobs.len()>=MAX_JOBS{return Err(conflict("model job history limit reached"));}
        }
        snapshot.jobs.insert(id.clone(),job.clone());
        persist(&root,&snapshot).map_err(|e|fault(StatusCode::INTERNAL_SERVER_ERROR,e))?;
    }
    spawn(root,shared,id,0);
    Ok(Json(json!(job)))
}
async fn jobs(State(engine):State<Arc<Engine>>)->Result<Json<Value>,Fault>{
    let (_,shared)=state(&engine.settings).await.map_err(|e|fault(StatusCode::INTERNAL_SERVER_ERROR,e))?;
    let mut jobs=shared.lock().await.jobs.values().cloned().collect::<Vec<_>>();
    jobs.sort_by(|a,b|b.started_at.cmp(&a.started_at));
    Ok(Json(json!(jobs)))
}
async fn job_action(State(engine):State<Arc<Engine>>,
    Path((id,action)):Path<(String,String)>)->Result<Json<Value>,Fault>{
    let (root,shared)=state(&engine.settings).await.map_err(|e|fault(StatusCode::INTERNAL_SERVER_ERROR,e))?;
    let mut snap=shared.lock().await;
    let job=snap.jobs.get_mut(&id).ok_or_else(||fault(StatusCode::NOT_FOUND,"model job not found"))?;
    match action.as_str(){
        "pause" if job.status=="downloading"=>job.status="paused".into(),
        "cancel" if matches!(job.status.as_str(),"paused"|"failed"|"downloading")=>job.status="cancelled".into(),
        "resume" if matches!(job.status.as_str(),"paused"|"failed"|"cancelled")=>{
            job.status="downloading".into();job.error.clear();
        }
        _=>return Err(conflict("this model job cannot perform the requested transition"))
    }
    job.generation=job.generation.saturating_add(1);
    if job.status=="downloading"{
        job.downloaded=disk_progress(&root,job);
    }
    let resumed=job.status=="downloading";
    let generation=job.generation;
    let output=json!(job);
    persist(&root,&snap).map_err(|e|fault(StatusCode::INTERNAL_SERVER_ERROR,e))?;
    drop(snap);
    if resumed{spawn(root,shared,id,generation);}
    Ok(Json(output))
}
async fn installed(State(engine):State<Arc<Engine>>)->Result<Json<Value>,Fault>{
    let (_,shared)=state(&engine.settings).await.map_err(|e|fault(StatusCode::INTERNAL_SERVER_ERROR,e))?;
    let mut models=shared.lock().await.installed.values().map(Installed::public)
        .collect::<Vec<_>>();
    models.sort_by(|a,b|a["label"].as_str().cmp(&b["label"].as_str()));
    Ok(Json(json!(models)))
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct ImportRequest{file_path:String,#[serde(default)]reference:bool}
async fn import(State(engine):State<Arc<Engine>>,Json(input):Json<ImportRequest>)
    ->Result<Json<Value>,Fault>{
    if !engine.settings.bind.ip().is_loopback(){
        return Err(fault(StatusCode::FORBIDDEN,"local file registration requires a loopback server"));
    }
    if !input.reference{return Err(bad("import only supports an explicit local-file reference"));}
    let path=fs::canonicalize(&input.file_path).map_err(|e|bad(format!("local model file: {e}")))?;
    let meta=fs::metadata(&path).map_err(|e|bad(e.to_string()))?;
    let format=if meta.is_file()&&path.extension().is_some_and(|e|e.eq_ignore_ascii_case("gguf")){
        "gguf"
    }else if meta.is_dir()&&path.join("config.json").is_file()
        &&fs::read_dir(&path).ok().is_some_and(|rows|
            rows.filter_map(Result::ok).any(|e|e.path().extension()
                .is_some_and(|extension|extension=="safetensors"))){
        "mlx"
    }else{return Err(bad("select a GGUF file or a complete MLX model directory"))};
    let (root,shared)=state(&engine.settings).await.map_err(|e|fault(StatusCode::INTERNAL_SERVER_ERROR,e))?;
    let id=format!("local-{}",Uuid::new_v4());
    let record=Installed{id:id.clone(),repo:path.file_name().unwrap_or_default()
        .to_string_lossy().to_string(),option_id:id.clone(),
        revision:"local".into(),source:"external".into(),format:format.into(),
        label:path.file_name().unwrap_or_default().to_string_lossy().to_string(),
        bytes:meta.len(),installed_at:crate::frontend_state::timestamp(now_ms()),
        external_path:Some(path.to_string_lossy().to_string()),
        files:vec![path.to_string_lossy().to_string()],ollama_model:None};
    {
        let mut snap=shared.lock().await;
        if snap.installed.len()>=128{return Err(conflict("model library full"));}
        snap.installed.insert(id,record.clone());
        persist(&root,&snap).map_err(|e|fault(StatusCode::INTERNAL_SERVER_ERROR,e))?;
    }
    Ok(Json(record.public()))
}
async fn remove(State(engine):State<Arc<Engine>>,
    Path(id):Path<String>)->Result<Json<Value>,Fault>{
    let (root,shared)=state(&engine.settings).await
        .map_err(|e|fault(StatusCode::INTERNAL_SERVER_ERROR,e))?;
    let record=shared.lock().await.installed.get(&id).cloned();
    let selected=crate::frontend_local_models::selected(&engine.settings);
    if let Some(ref installed)=record{
        if installed.ollama_model.as_deref()==selected.as_deref(){
            return Err(conflict("unload the currently active model before removing it"));
        }
    }else if selected.as_deref()==Some(id.as_str()){
        return Err(conflict("unload the currently active Ollama model before deleting it"));
    }
    let target=if let Some(ref installed)=record{
        installed.ollama_model.as_deref()
    }else{Some(id.as_str())};
    if let Some(model)=target{
        if model.len()>240||model.chars().any(char::is_control){
            return Err(bad("invalid model identifier"));
        }
        let tags=crate::frontend_local_models::ollama_get("/api/tags").await
            .map_err(|e|fault(StatusCode::BAD_GATEWAY,e))?;
        let present=tags["models"].as_array().is_some_and(|entries|entries.iter()
            .any(|e|e["name"].as_str()==Some(model)
                ||e["model"].as_str()==Some(model)));
        if present{
            let endpoint=crate::frontend_local_models::endpoint().map_err(bad)?;
            let response=reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(90))
                .redirect(reqwest::redirect::Policy::none()).build()
                .map_err(|e|fault(StatusCode::INTERNAL_SERVER_ERROR,e.to_string()))?
                .delete(format!("{endpoint}/api/delete"))
                .json(&json!({"model":model})).send().await
                .map_err(|e|fault(StatusCode::BAD_GATEWAY,e.to_string()))?;
            if !response.status().is_success(){
                return Err(fault(StatusCode::BAD_GATEWAY,
                    format!("Ollama model deletion returned HTTP {}",response.status())));
            }
        }else if record.is_none(){
            return Err(fault(StatusCode::NOT_FOUND,"Ollama model is no longer installed"));
        }
    }
    let Some(installed)=record else{return Ok(Json(json!({"ok":true})))};
    let mut snap=shared.lock().await;
    if snap.installed.remove(&id).is_some(){
        persist(&root,&snap).map_err(|e|fault(StatusCode::INTERNAL_SERVER_ERROR,e))?;
    }
    drop(snap);
    if installed.source=="huggingface"{
        if let Some(job_id)=id.strip_prefix("hf-"){
            let dir=job_dir(&root,job_id).map_err(bad)?;
            if dir.exists(){async_fs::remove_dir_all(dir).await.map_err(|e|
                fault(StatusCode::INTERNAL_SERVER_ERROR,e.to_string()))?;}
        }
    }
    Ok(Json(json!({"ok":true})))
}
pub(crate) async fn find(settings:&Settings,id:&str)->Result<Option<Installed>,String>{
    let (_,shared)=state(settings).await?;
    let entry=shared.lock().await.installed.get(id).cloned();
    Ok(entry)
}

pub(crate) async fn list(settings:&Settings)->Result<Vec<Value>,String>{
    let (_,shared)=state(settings).await?;
    let mut models=shared.lock().await.installed.values().map(Installed::public)
        .collect::<Vec<_>>();
    models.sort_by(|a,b|a["label"].as_str().cmp(&b["label"].as_str()));
    Ok(models)
}
static IMPORT_LOCK:OnceLock<Mutex<()>>=OnceLock::new();
/// Resolve the UI's managed GGUF ID into a model running on the existing
/// loopback Ollama adapter. The native Rust agent kernel remains unchanged.
pub(crate) async fn prepare_for_load(settings:&Settings,id:&str)
    ->Result<Option<String>,String>{
    let Some(initial)=find(settings,id).await? else{return Ok(None)};
    if initial.format!="gguf"{
        return Err("This model is MLX; a native MLX runtime is not installed. The GGUF/Ollama runner cannot execute it".into());
    }
    let lock=IMPORT_LOCK.get_or_init(||Mutex::new(()));
    let _held=lock.lock().await;
    let Some(record)=find(settings,id).await? else{
        return Err("model was removed during preparation".into());
    };
    if let Some(alias)=record.ollama_model.as_deref(){
        if let Ok(tags)=crate::frontend_local_models::ollama_get("/api/tags").await{
            if tags["models"].as_array().is_some_and(|rows|rows.iter().any(|r|
                r["name"].as_str()==Some(alias)||r["model"].as_str()==Some(alias))){
                return Ok(Some(alias.to_owned()));
            }
        }
    }
    if record.files.is_empty()||record.files.len()>MAX_FILES{
        return Err("model has no registered files".into());
    }
    let mut paths=Vec::new();
    for file in &record.files{
        let path=fs::canonicalize(file).map_err(|e|format!("local GGUF unavailable: {e}"))?;
        if !path.is_file()||path.extension().is_none_or(|v|
            !v.eq_ignore_ascii_case("gguf")){
            return Err("GGUF files are missing or incomplete".into());
        }
        paths.push(path);
    }
    let first=paths.first().ok_or("GGUF model has no first shard")?;
    let reference=if paths.len()==1{first.clone()}else{
        let parent=first.parent().ok_or("split GGUF path has no parent")?;
        if paths.iter().any(|path|path.parent()!=Some(parent)){
            return Err("split GGUF shards must share one directory".into());
        }
        let name=first.file_name().and_then(|v|v.to_str())
            .ok_or("split GGUF filename is invalid")?;
        let (prefix,rest)=name.split_once("-00001-of-")
            .ok_or("split GGUF first shard is invalid")?;
        let expected=rest.strip_suffix(".gguf")
            .and_then(|s|s.parse::<usize>().ok())
            .ok_or("split GGUF shard count is invalid")?;
        if expected!=paths.len()||expected>256{
            return Err("split GGUF shard count mismatch".into());
        }
        for (i,path) in paths.iter().enumerate(){
            let name=path.file_name().and_then(|v|v.to_str()).unwrap_or("");
            if name!=format!("{prefix}-{:05}-of-{expected:05}.gguf",i+1){
                return Err("split GGUF missing or reordered shard".into());
            }
        }
        parent.join(format!("{prefix}-*.gguf"))
    };
    let source=reference.to_string_lossy();
    if source.chars().any(char::is_control)||source.contains('"'){
        return Err("GGUF file path contains unsupported control/quote characters".into());
    }
    let model_name=format!("dominion-{}",id.replace('-',"").replace(':',""));
    let directory=base(settings).join("modelfiles");
    async_fs::create_dir_all(&directory).await.map_err(|e|e.to_string())?;
    let file=directory.join(format!("{model_name}.Modelfile"));
    async_fs::write(&file,format!("FROM \"{source}\"\n")).await
        .map_err(|e|e.to_string())?;
    let endpoint=crate::frontend_local_models::endpoint()?;
    let output=tokio::time::timeout(std::time::Duration::from_secs(600),
        tokio::process::Command::new("ollama").args(["create",&model_name,"-f"])
            .arg(&file).env("OLLAMA_HOST",&endpoint)
            .kill_on_drop(true).output()).await
        .map_err(|_|"Ollama GGUF registration exceeded 10 minutes")?
        .map_err(|e|format!("Ollama CLI unavailable: {e}"))?;
    if !output.status.success(){
        return Err(format!("Ollama could not import the GGUF: {}",
            String::from_utf8_lossy(&output.stderr).chars().take(400).collect::<String>()));
    }
    let (root,shared)=state(settings).await?;
    let mut snap=shared.lock().await;
    if let Some(stored)=snap.installed.get_mut(id){
        stored.ollama_model=Some(model_name.clone());
        persist(&root,&snap)?;
    }
    Ok(Some(model_name))
}

pub fn routes()->Router<Arc<Engine>>{
    Router::new()
        .route("/model-library/jobs",get(jobs))
        .route("/model-library/jobs/{id}/{action}",post(job_action))
        .route("/model-library/download",post(download))
        .route("/model-library/import",post(import))
        .route("/model-library/managed-installed",get(installed))
        .route("/model-library/installed/{id}",axum::routing::delete(remove))
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn pinned_download_does_not_accept_traversal_or_untrusted_hub_revision(){
        assert!(valid_repo("Qwen/Qwen3-GGUF"));
        assert!(!valid_repo("Qwen/../secret"));
        assert!(!valid_repo("Qwen/a/b"));
        assert!(!valid_file("../../secret.gguf"));
        assert!(!valid_file("file\\evil.gguf"));
        assert!(valid_file("sub/model.Q4_K_M.gguf"));
        assert!(valid_revision("0123456789abcdef0123456789abcdef01234567"));
        assert!(!valid_revision("main"));
    }
    #[test]
    fn job_status_and_installed_receipts_match_ui_contract(){
        let j=Job{id:Uuid::new_v4().to_string(),repo:"r/m".into(),
            option_id:"a.gguf".into(),revision:"sha".into(),format:"gguf".into(),
            status:"downloading".into(),downloaded:10,total:100,current:"a.gguf".into(),
            error:String::new(),installed_id:None,started_at:"time".into(),
            files:vec!["a.gguf".into()],generation:0,sizes:vec![100]};
        let value=json!(&j);
        assert_eq!(value["optionId"],"a.gguf");
        assert_eq!(value["downloaded"],10);
        assert_eq!(value["installedId"],Value::Null);
    }
}
